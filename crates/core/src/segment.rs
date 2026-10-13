use crate::error::{Error, Result};

/// Never create a segment smaller than this by splitting. Below it, the cost
/// of opening another connection outweighs the gain.
pub(crate) const MIN_SPLIT: u64 = 1024 * 1024;

/// End offset used while the file size is unknown.
const UNKNOWN_END: u64 = u64::MAX;

/// A byte range `[start, end)` of the file, downloaded by one connection at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub start: u64,
    /// Exclusive. Shrinks when another connection takes over the second half.
    pub end: u64,
    /// Bytes before this have arrived from the network (some may still be buffered).
    pub received: u64,
    /// Bytes before this are written to the file.
    pub written: u64,
    /// A connection is working on this segment.
    pub active: bool,
}

impl Segment {
    fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end,
            received: start,
            written: start,
            active: false,
        }
    }

    fn is_done(&self) -> bool {
        self.written >= self.end
    }
}

/// All segments of one download. Kept behind a mutex and touched once per
/// received chunk, so every method is cheap.
#[derive(Debug, Default)]
pub(crate) struct Segments {
    list: Vec<Segment>,
    /// `None` while the size is unknown.
    size: Option<u64>,
    /// Range requests work, so idle connections may split busy segments.
    splittable: bool,
    /// Connections still working on the download. Kept here, under the same
    /// lock as the segments, so a segment handed back by a leaving connection
    /// is always seen by one that stays.
    workers: usize,
}

impl Segments {
    /// A fresh download over `connections` connections, without making
    /// segments smaller than [`MIN_SPLIT`].
    pub fn split(size: u64, connections: usize) -> Self {
        let n = (size / MIN_SPLIT).clamp(1, connections.max(1) as u64);
        let part = size / n;
        let list = (0..n)
            .map(|i| Segment::new(i * part, if i == n - 1 { size } else { (i + 1) * part }))
            .collect();
        Self {
            list,
            size: Some(size),
            splittable: true,
            workers: 0,
        }
    }

    /// Continues a download from the ranges that were still missing.
    pub fn resume(size: u64, remaining: &[(u64, u64)]) -> Self {
        let list = remaining
            .iter()
            .filter(|(s, e)| s < e && *e <= size)
            .map(|&(s, e)| Segment::new(s, e))
            .collect();
        Self {
            list,
            size: Some(size),
            splittable: true,
            workers: 0,
        }
    }

    /// One connection from the start, for servers without range support.
    pub fn single(size: Option<u64>) -> Self {
        Self {
            list: vec![Segment::new(0, size.unwrap_or(UNKNOWN_END))],
            size,
            splittable: false,
            workers: 0,
        }
    }

    pub fn set_workers(&mut self, workers: usize) {
        self.workers = workers;
    }

    /// Like [`Segments::take`], but a connection that gets nothing is counted as gone.
    pub fn take_or_leave(&mut self) -> Option<usize> {
        let taken = self.take();
        if taken.is_none() {
            self.workers -= 1;
        }
        taken
    }

    /// Hands segment `i` back and removes the connection, unless it is the
    /// last one. Returns whether it left.
    pub fn leave(&mut self, i: usize) -> bool {
        if self.workers <= 1 {
            return false;
        }
        self.release(i);
        self.workers -= 1;
        true
    }

    /// Hands a connection a segment to work on: an idle unfinished one, or
    /// else the second half of the largest busy one.
    pub fn take(&mut self) -> Option<usize> {
        if let Some(i) = self.list.iter().position(|s| !s.active && !s.is_done()) {
            self.list[i].active = true;
            return Some(i);
        }
        if !self.splittable {
            return None;
        }
        let (i, remaining) = self
            .list
            .iter()
            .enumerate()
            .filter(|(_, s)| s.active)
            .map(|(i, s)| (i, s.end - s.received))
            .max_by_key(|&(_, r)| r)?;
        if remaining < 2 * MIN_SPLIT {
            return None;
        }
        let busy = &mut self.list[i];
        let mid = busy.received + remaining / 2;
        let end = std::mem::replace(&mut busy.end, mid);
        self.list.push(Segment {
            active: true,
            ..Segment::new(mid, end)
        });
        Some(self.list.len() - 1)
    }

    /// Starts (or retries) a segment: drops data that was received but not
    /// written, and returns the range still to fetch.
    pub fn restart(&mut self, i: usize) -> (u64, u64) {
        let s = &mut self.list[i];
        s.received = s.written;
        (s.written, s.end)
    }

    /// Accepts up to `len` newly received bytes. Returns how many belong to
    /// this segment (fewer if it was split meanwhile) and whether it is now complete.
    pub fn claim(&mut self, i: usize, len: u64) -> (u64, bool) {
        let s = &mut self.list[i];
        let take = len.min(s.end - s.received);
        s.received += take;
        (take, s.received >= s.end)
    }

    /// Records that `len` more bytes of the segment are on disk.
    pub fn commit(&mut self, i: usize, len: u64) {
        self.list[i].written += len;
    }

    /// Called when the server closes the response normally.
    pub fn finish_stream(&mut self, i: usize) -> Result<()> {
        let s = &mut self.list[i];
        if s.end == UNKNOWN_END {
            s.end = s.received;
            self.size = Some(s.end);
            Ok(())
        } else if s.received >= s.end {
            Ok(())
        } else {
            Err(Error::Incomplete)
        }
    }

    /// The connection stopped working on this segment.
    pub fn release(&mut self, i: usize) {
        let s = &mut self.list[i];
        s.active = false;
        s.received = s.written;
    }

    pub fn get(&self, i: usize) -> &Segment {
        &self.list[i]
    }

    pub fn is_done(&self) -> bool {
        self.list.iter().all(Segment::is_done)
    }

    /// Byte ranges not yet on disk, for the resume file.
    pub fn remaining(&self) -> Vec<(u64, u64)> {
        self.list.iter().filter(|s| !s.is_done()).map(|s| (s.written, s.end)).collect()
    }

    /// Bytes received so far, including ones finished before a resume.
    pub fn downloaded(&self) -> u64 {
        match self.size {
            Some(size) => size - self.list.iter().map(|s| s.end.min(size) - s.received.min(size)).sum::<u64>(),
            None => self.list.iter().map(|s| s.received - s.start).sum(),
        }
    }

    pub fn active(&self) -> usize {
        self.list.iter().filter(|s| s.active).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Segment> {
        self.list.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = MIN_SPLIT;

    #[test]
    fn split_covers_the_file() {
        let segs = Segments::split(10 * MB + 7, 4);
        let ranges: Vec<_> = segs.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0].0, 0);
        assert_eq!(ranges[3].1, 10 * MB + 7);
        assert!(ranges.windows(2).all(|w| w[0].1 == w[1].0));
    }

    #[test]
    fn small_files_get_fewer_segments() {
        assert_eq!(Segments::split(3 * MB, 8).iter().count(), 3);
        assert_eq!(Segments::split(100, 8).iter().count(), 1);
        let empty = Segments::split(0, 8);
        assert!(empty.is_done());
    }

    #[test]
    fn idle_connection_splits_largest_segment() {
        let mut segs = Segments::split(8 * MB, 1);
        let a = segs.take().unwrap();
        assert_eq!(segs.claim(a, 2 * MB), (2 * MB, false));
        // 6 MB left in `a`; the new connection takes the second half.
        let b = segs.take().unwrap();
        assert_eq!((segs.get(b).start, segs.get(b).end), (5 * MB, 8 * MB));
        assert_eq!(segs.get(a).end, 5 * MB);
        // `a` can't claim past its new end.
        assert_eq!(segs.claim(a, 4 * MB), (3 * MB, true));
    }

    #[test]
    fn no_split_below_minimum() {
        let mut segs = Segments::split(3 * MB / 2, 1);
        segs.take().unwrap();
        assert_eq!(segs.take(), None);
    }

    #[test]
    fn retry_drops_unwritten_bytes() {
        let mut segs = Segments::split(4 * MB, 1);
        let a = segs.take().unwrap();
        segs.claim(a, 300);
        segs.commit(a, 100);
        assert_eq!(segs.restart(a), (100, 4 * MB));
        assert_eq!(segs.downloaded(), 100);
    }

    #[test]
    fn resume_ranges_round_trip() {
        let mut segs = Segments::split(4 * MB, 2);
        let a = segs.take().unwrap();
        segs.claim(a, MB);
        segs.commit(a, MB);
        segs.release(a);
        let remaining = segs.remaining();
        assert_eq!(remaining, vec![(MB, 2 * MB), (2 * MB, 4 * MB)]);
        let resumed = Segments::resume(4 * MB, &remaining);
        assert_eq!(resumed.downloaded(), MB);
    }

    #[test]
    fn last_connection_never_leaves() {
        let mut segs = Segments::split(4 * MB, 2);
        segs.set_workers(2);
        let a = segs.take_or_leave().unwrap();
        let b = segs.take_or_leave().unwrap();
        assert!(segs.leave(a));
        // `a`'s segment goes back to the pool for `b` to pick up afterwards.
        assert_eq!(segs.take(), Some(a));
        assert!(!segs.leave(b));
    }

    #[test]
    fn unknown_size_ends_with_stream() {
        let mut segs = Segments::single(None);
        let a = segs.take().unwrap();
        segs.claim(a, 500);
        segs.commit(a, 500);
        segs.finish_stream(a).unwrap();
        assert!(segs.is_done());
        assert_eq!(segs.downloaded(), 500);
    }

    #[test]
    fn early_close_is_an_error() {
        let mut segs = Segments::single(Some(1000));
        let a = segs.take().unwrap();
        segs.claim(a, 10);
        assert!(matches!(segs.finish_stream(a), Err(Error::Incomplete)));
    }
}
