//! Joins a video-only and an audio-only fragmented MP4 (as YouTube serves
//! them) into one regular MP4 with both tracks.
//!
//! Fragmented MP4 is a streaming format: every few seconds a small header
//! describes the next few samples. Browsers play it, but many desktop
//! players don't, or freeze the picture while the sound plays on. So the
//! fragments are taken apart into their samples (video frames and audio
//! packets), and written as a regular MP4: one header (`moov`) with complete
//! sample tables for both tracks, then all the media (`mdat`), interleaved in
//! short chunks. Nothing is re-encoded.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Header boxes bigger than this aren't from a sane file.
const MAX_SMALL_BOX: u64 = 64 * 1024 * 1024;
/// Media is interleaved in chunks of about this much playing time per track.
const CHUNK_SECONDS: f64 = 0.5;
/// `sample_is_non_sync_sample` in sample flags: not a keyframe.
const NON_SYNC: u32 = 0x0001_0000;

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("can't join video and sound: {what}"))
}

/// A box inside a byte slice: its type and where its contents are.
struct Child {
    kind: [u8; 4],
    start: usize,
    body: usize,
    end: usize,
}

fn children(data: &[u8]) -> io::Result<Vec<Child>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().expect("4 bytes")) as usize;
        let kind: [u8; 4] = data[pos + 4..pos + 8].try_into().expect("4 bytes");
        let (size, header) = match size32 {
            0 => (data.len() - pos, 8),
            1 => {
                let large = data.get(pos + 8..pos + 16).ok_or_else(|| invalid("box too short"))?;
                (u64::from_be_bytes(large.try_into().expect("8 bytes")) as usize, 16)
            }
            n => (n, 8),
        };
        if size < header || pos + size > data.len() {
            return Err(invalid("box size out of range"));
        }
        out.push(Child {
            kind,
            start: pos,
            body: pos + header,
            end: pos + size,
        });
        pos += size;
    }
    Ok(out)
}

fn find<'a>(data: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    let (first, rest) = path.split_first()?;
    let child = children(data).ok()?.into_iter().find(|c| &c.kind == *first)?;
    let body = &data[child.body..child.end];
    if rest.is_empty() { Some(body) } else { find(body, rest) }
}

fn make_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// A full box's body: version, flags, then `rest`.
fn full_box(kind: &[u8; 4], version: u8, rest: &[u8]) -> Vec<u8> {
    let mut body = vec![version, 0, 0, 0];
    body.extend_from_slice(rest);
    make_box(kind, &body)
}

/// Given a child box's type and contents, its new contents, or `None` to keep it.
type EditBox<'a> = dyn FnMut(&[u8; 4], &[u8]) -> io::Result<Option<Vec<u8>>> + 'a;

/// Rewrites child boxes inside `data` (one level deep) with `edit`. Boxes
/// edited to nothing are left out if they're of a kind a regular MP4 drops.
fn edit_children(data: &[u8], edit: &mut EditBox<'_>) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    for child in children(data)? {
        match edit(&child.kind, &data[child.body..child.end])? {
            Some(body) if body.is_empty() && is_droppable(&child.kind) => {}
            Some(body) => out.extend_from_slice(&make_box(&child.kind, &body)),
            None => out.extend_from_slice(&data[child.start..child.end]),
        }
    }
    Ok(out)
}

/// Boxes left out of the joined file: the fragment defaults, and the
/// fragment-time tables that only describe fragmented files.
fn is_droppable(kind: &[u8; 4]) -> bool {
    matches!(
        kind,
        b"mvex" | b"stts" | b"ctts" | b"stsc" | b"stsz" | b"stz2" | b"stco" | b"co64" | b"stss" | b"sdtp" | b"sgpd" | b"sbgp"
    )
}

fn u32_at(data: &[u8], at: usize) -> io::Result<u32> {
    Ok(u32::from_be_bytes(
        data.get(at..at + 4).ok_or_else(|| invalid("box too short"))?.try_into().expect("4 bytes"),
    ))
}

fn u64_at(data: &[u8], at: usize) -> io::Result<u64> {
    Ok(u64::from_be_bytes(
        data.get(at..at + 8).ok_or_else(|| invalid("box too short"))?.try_into().expect("8 bytes"),
    ))
}

fn version(body: &[u8]) -> u8 {
    body.first().copied().unwrap_or(0)
}

fn flags(body: &[u8]) -> u32 {
    u32::from_be_bytes([0, body[1], body[2], body[3]])
}

/// Writes a duration into a box that has one at `v0_at` (version 0, 32 bits)
/// or `v1_at` (version 1, 64 bits).
fn set_duration(body: &mut [u8], v0_at: usize, v1_at: usize, duration: u64) -> io::Result<()> {
    if version(body) == 1 {
        body.get_mut(v1_at..v1_at + 8)
            .ok_or_else(|| invalid("box too short"))?
            .copy_from_slice(&duration.to_be_bytes());
    } else {
        let short = u32::try_from(duration).unwrap_or(u32::MAX);
        body.get_mut(v0_at..v0_at + 4)
            .ok_or_else(|| invalid("box too short"))?
            .copy_from_slice(&short.to_be_bytes());
    }
    Ok(())
}

/// One video frame or audio packet, and where its bytes are in the input.
#[derive(Debug, Clone, Copy)]
struct Sample {
    offset: u64,
    size: u32,
    duration: u32,
    sync: bool,
    /// Composition (display) time offset.
    cto: i32,
}

/// A top-level box of an input file, without loading media data.
struct TopBox {
    kind: [u8; 4],
    offset: u64,
    size: u64,
    header: u64,
}

fn top_boxes(file: &mut File) -> io::Result<Vec<TopBox>> {
    let len = file.metadata()?.len();
    let mut boxes = Vec::new();
    let mut offset = 0;
    while offset + 8 <= len {
        file.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 16];
        file.read_exact(&mut head[..8])?;
        let size32 = u32::from_be_bytes(head[..4].try_into().expect("4 bytes"));
        let kind: [u8; 4] = head[4..8].try_into().expect("4 bytes");
        let (size, header) = match size32 {
            0 => (len - offset, 8),
            1 => {
                file.read_exact(&mut head[8..16])?;
                (u64::from_be_bytes(head[8..16].try_into().expect("8 bytes")), 16)
            }
            n => (u64::from(n), 8),
        };
        if size < header || offset + size > len {
            return Err(invalid("file is cut short"));
        }
        boxes.push(TopBox { kind, offset, size, header });
        offset += size;
    }
    Ok(boxes)
}

fn read_box(file: &mut File, top: &TopBox) -> io::Result<Vec<u8>> {
    if top.size > MAX_SMALL_BOX {
        return Err(invalid("header box too large"));
    }
    file.seek(SeekFrom::Start(top.offset))?;
    let mut data = vec![0u8; top.size as usize];
    file.read_exact(&mut data)?;
    Ok(data)
}

/// Defaults for a track's samples, from its `trex`.
#[derive(Clone, Copy)]
struct Defaults {
    duration: u32,
    size: u32,
    flags: u32,
}

/// One input file: its track's header and all its samples.
struct Input {
    file: File,
    /// The input's `moov` contents (without its own header).
    moov: Vec<u8>,
    timescale: u32,
    samples: Vec<Sample>,
}

impl Input {
    fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let boxes = top_boxes(&mut file)?;
        let moov_box = boxes.iter().find(|b| &b.kind == b"moov").ok_or_else(|| invalid("no header (moov)"))?;
        let moov = read_box(&mut file, moov_box)?[moov_box.header as usize..].to_vec();
        let mdhd = find(&moov, &[b"trak", b"mdia", b"mdhd"]).ok_or_else(|| invalid("no track timescale"))?;
        let timescale = u32_at(mdhd, if version(mdhd) == 1 { 20 } else { 12 })?.max(1);
        let trex = find(&moov, &[b"mvex", b"trex"]).ok_or_else(|| invalid("not a fragmented file (no trex)"))?;
        let defaults = Defaults {
            duration: u32_at(trex, 12)?,
            size: u32_at(trex, 16)?,
            flags: u32_at(trex, 20)?,
        };
        let mut samples = Vec::new();
        for moof in boxes.iter().filter(|b| &b.kind == b"moof") {
            let data = read_box(&mut file, moof)?;
            read_fragment(&data[moof.header as usize..], moof.offset, defaults, &mut samples)?;
        }
        if samples.is_empty() {
            return Err(invalid("no media"));
        }
        Ok(Self {
            file,
            moov,
            timescale,
            samples,
        })
    }

    fn duration(&self) -> u64 {
        self.samples.iter().map(|s| u64::from(s.duration)).sum()
    }
}

/// Adds the samples a `moof` describes, with where their bytes are in the file.
fn read_fragment(moof: &[u8], moof_offset: u64, trex: Defaults, samples: &mut Vec<Sample>) -> io::Result<()> {
    for traf in children(moof)?.iter().filter(|c| &c.kind == b"traf") {
        let traf = &moof[traf.body..traf.end];
        let tfhd = find(traf, &[b"tfhd"]).ok_or_else(|| invalid("fragment without tfhd"))?;
        let tf = flags(tfhd);
        let mut at = 8;
        let mut base = moof_offset;
        if tf & 0x1 != 0 {
            base = u64_at(tfhd, at)?;
            at += 8;
        }
        if tf & 0x2 != 0 {
            at += 4;
        }
        let mut defaults = trex;
        if tf & 0x8 != 0 {
            defaults.duration = u32_at(tfhd, at)?;
            at += 4;
        }
        if tf & 0x10 != 0 {
            defaults.size = u32_at(tfhd, at)?;
            at += 4;
        }
        if tf & 0x20 != 0 {
            defaults.flags = u32_at(tfhd, at)?;
        }

        // Each run continues where the previous one's data ended, unless it says otherwise.
        let mut next = base;
        for trun in children(traf)?.iter().filter(|c| &c.kind == b"trun") {
            let trun = &traf[trun.body..trun.end];
            let rf = flags(trun);
            let signed_cto = version(trun) == 1;
            let count = u32_at(trun, 4)?;
            let mut at = 8;
            if rf & 0x1 != 0 {
                let data_offset = u32_at(trun, at)? as i32;
                next = (base as i64 + i64::from(data_offset)) as u64;
                at += 4;
            }
            let mut first_flags = None;
            if rf & 0x4 != 0 {
                first_flags = Some(u32_at(trun, at)?);
                at += 4;
            }
            for i in 0..count {
                let mut sample = Sample {
                    offset: next,
                    size: defaults.size,
                    duration: defaults.duration,
                    sync: true,
                    cto: 0,
                };
                let mut sample_flags = if i == 0 { first_flags.unwrap_or(defaults.flags) } else { defaults.flags };
                if rf & 0x100 != 0 {
                    sample.duration = u32_at(trun, at)?;
                    at += 4;
                }
                if rf & 0x200 != 0 {
                    sample.size = u32_at(trun, at)?;
                    at += 4;
                }
                if rf & 0x400 != 0 {
                    sample_flags = u32_at(trun, at)?;
                    at += 4;
                }
                if rf & 0x800 != 0 {
                    let raw = u32_at(trun, at)?;
                    sample.cto = if signed_cto {
                        raw as i32
                    } else {
                        i32::try_from(raw).unwrap_or(i32::MAX)
                    };
                    at += 4;
                }
                sample.sync = sample_flags & NON_SYNC == 0;
                next += u64::from(sample.size);
                samples.push(sample);
            }
        }
    }
    Ok(())
}

/// A run of samples of one track that go together in the output.
struct Chunk {
    track: usize,
    first: usize,
    count: usize,
    /// When it starts playing, in seconds.
    start: f64,
}

fn chunks(inputs: &[Input]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (track, input) in inputs.iter().enumerate() {
        let per_chunk = (CHUNK_SECONDS * f64::from(input.timescale)) as u64;
        let mut time = 0u64;
        let mut first = 0;
        let mut chunk_time = 0u64;
        for (i, sample) in input.samples.iter().enumerate() {
            if i > first && time - chunk_time >= per_chunk {
                chunks.push(Chunk {
                    track,
                    first,
                    count: i - first,
                    start: chunk_time as f64 / f64::from(input.timescale),
                });
                first = i;
                chunk_time = time;
            }
            time += u64::from(sample.duration);
        }
        chunks.push(Chunk {
            track,
            first,
            count: input.samples.len() - first,
            start: chunk_time as f64 / f64::from(input.timescale),
        });
    }
    chunks.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.track.cmp(&b.track)));
    chunks
}

/// Run-length pairs `(count, value)`.
fn runs<T: PartialEq + Copy>(values: impl Iterator<Item = T>) -> Vec<(u32, T)> {
    let mut out: Vec<(u32, T)> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some((count, last)) if *last == value => *count += 1,
            _ => out.push((1, value)),
        }
    }
    out
}

/// The sample tables of a track: timing, display offsets, sizes, chunks and keyframes.
fn sample_tables(input: &Input, track_chunks: &[(&Chunk, u64)]) -> Vec<u8> {
    let samples = &input.samples;
    let mut out = Vec::new();

    let stts = runs(samples.iter().map(|s| s.duration));
    let mut body = (stts.len() as u32).to_be_bytes().to_vec();
    for (count, duration) in stts {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&duration.to_be_bytes());
    }
    out.extend_from_slice(&full_box(b"stts", 0, &body));

    if samples.iter().any(|s| s.cto != 0) {
        let ctts = runs(samples.iter().map(|s| s.cto));
        let negative = ctts.iter().any(|(_, cto)| *cto < 0);
        let mut body = (ctts.len() as u32).to_be_bytes().to_vec();
        for (count, cto) in ctts {
            body.extend_from_slice(&count.to_be_bytes());
            body.extend_from_slice(&cto.to_be_bytes());
        }
        out.extend_from_slice(&full_box(b"ctts", u8::from(negative), &body));
    }

    let stsc = runs(track_chunks.iter().map(|(c, _)| c.count as u32));
    let mut body = Vec::new();
    let mut entries = 0u32;
    let mut chunk_number = 1u32;
    for (count, per_chunk) in stsc {
        body.extend_from_slice(&chunk_number.to_be_bytes());
        body.extend_from_slice(&per_chunk.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        chunk_number += count;
        entries += 1;
    }
    out.extend_from_slice(&full_box(b"stsc", 0, &[entries.to_be_bytes().to_vec(), body].concat()));

    let mut body = 0u32.to_be_bytes().to_vec();
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        body.extend_from_slice(&sample.size.to_be_bytes());
    }
    out.extend_from_slice(&full_box(b"stsz", 0, &body));

    let mut body = (track_chunks.len() as u32).to_be_bytes().to_vec();
    for (_, offset) in track_chunks {
        body.extend_from_slice(&offset.to_be_bytes());
    }
    out.extend_from_slice(&full_box(b"co64", 0, &body));

    if samples.iter().any(|s| !s.sync) {
        let sync: Vec<u32> = samples.iter().enumerate().filter(|(_, s)| s.sync).map(|(i, _)| i as u32 + 1).collect();
        let mut body = (sync.len() as u32).to_be_bytes().to_vec();
        for n in sync {
            body.extend_from_slice(&n.to_be_bytes());
        }
        out.extend_from_slice(&full_box(b"stss", 0, &body));
    }
    out
}

/// The input's track, numbered `id`, with complete sample tables and its
/// length filled in (`movie_duration` in the joined file's timescale).
fn track(input: &Input, id: u32, movie_duration: u64, tables: &[u8]) -> io::Result<Vec<u8>> {
    let trak = find(&input.moov, &[b"trak"]).ok_or_else(|| invalid("no track"))?;
    let media_duration = input.duration();
    let body = edit_children(trak, &mut |kind, body| match kind {
        b"tkhd" => {
            let mut body = body.to_vec();
            let at = if version(&body) == 1 { 20 } else { 12 };
            body.get_mut(at..at + 4)
                .ok_or_else(|| invalid("tkhd"))?
                .copy_from_slice(&id.to_be_bytes());
            set_duration(&mut body, 20, 28, movie_duration)?;
            Ok(Some(body))
        }
        b"mdia" => Ok(Some(edit_children(body, &mut |kind, body| match kind {
            b"mdhd" => {
                let mut body = body.to_vec();
                set_duration(&mut body, 16, 24, media_duration)?;
                Ok(Some(body))
            }
            b"minf" => Ok(Some(edit_children(body, &mut |kind, body| match kind {
                b"stbl" => {
                    // The sample description stays; the fragment-era tables go.
                    let mut stbl = edit_children(body, &mut |kind, _| Ok(is_droppable(kind).then(Vec::new)))?;
                    stbl.extend_from_slice(tables);
                    Ok(Some(stbl))
                }
                _ => Ok(None),
            })?)),
            _ => Ok(None),
        })?)),
        _ => Ok(None),
    })?;
    Ok(make_box(b"trak", &body))
}

fn movie_header(video: &Input, durations: &[u64]) -> io::Result<(Vec<u8>, u32)> {
    let mut mvhd = find(&video.moov, &[b"mvhd"]).ok_or_else(|| invalid("no movie header"))?.to_vec();
    let timescale = u32_at(&mvhd, if version(&mvhd) == 1 { 20 } else { 12 })?.max(1);
    let longest = durations.iter().copied().max().unwrap_or(0);
    set_duration(&mut mvhd, 16, 24, longest)?;
    // next_track_ID is the last field.
    let len = mvhd.len();
    mvhd.get_mut(len - 4..)
        .ok_or_else(|| invalid("mvhd"))?
        .copy_from_slice(&3u32.to_be_bytes());
    Ok((make_box(b"mvhd", &mvhd), timescale))
}

fn header(inputs: &[Input], chunk_offsets: &[Vec<(&Chunk, u64)>]) -> io::Result<Vec<u8>> {
    let movie_timescale = movie_header(&inputs[0], &[])?.1;
    let durations: Vec<u64> = inputs
        .iter()
        .map(|i| (i.duration() as u128 * u128::from(movie_timescale) / u128::from(i.timescale)) as u64)
        .collect();
    let (mvhd, _) = movie_header(&inputs[0], &durations)?;
    let mut moov = mvhd;
    for (index, input) in inputs.iter().enumerate() {
        let tables = sample_tables(input, &chunk_offsets[index]);
        moov.extend_from_slice(&track(input, index as u32 + 1, durations[index], &tables)?);
    }
    Ok(make_box(b"moov", &moov))
}

fn file_type() -> Vec<u8> {
    let mut body = b"isom".to_vec();
    body.extend_from_slice(&0x200u32.to_be_bytes());
    body.extend_from_slice(b"isomiso2avc1mp41");
    make_box(b"ftyp", &body)
}

fn copy_range(file: &mut File, offset: u64, len: u64, out: &mut impl Write) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    let copied = io::copy(&mut BufReader::with_capacity(1024 * 1024, Read::by_ref(file).take(len)), out)?;
    if copied != len {
        return Err(invalid("file is cut short"));
    }
    Ok(())
}

/// Writes `video` and `audio` into one regular MP4 at `out`.
pub(crate) fn join(video: &Path, audio: &Path, out: &Path) -> io::Result<()> {
    let mut inputs = [Input::open(video)?, Input::open(audio)?];
    let order = chunks(&inputs);
    let chunk_size = |c: &Chunk| -> u64 {
        inputs[c.track].samples[c.first..c.first + c.count]
            .iter()
            .map(|s| u64::from(s.size))
            .sum()
    };

    // The header's size doesn't depend on the offsets in it, so build it once
    // with placeholders to learn where the media will start.
    let place = |media_start: u64| {
        let mut offsets: Vec<Vec<(&Chunk, u64)>> = vec![Vec::new(), Vec::new()];
        let mut position = media_start;
        for chunk in &order {
            offsets[chunk.track].push((chunk, position));
            position += chunk_size(chunk);
        }
        offsets
    };
    let ftyp = file_type();
    let probe = header(&inputs, &place(0))?;
    // `mdat` with a 64-bit size, so files over 4 GB work too.
    let media_start = (ftyp.len() + probe.len() + 16) as u64;
    let moov = header(&inputs, &place(media_start))?;
    let media_len: u64 = order.iter().map(chunk_size).sum();

    let mut writer = BufWriter::with_capacity(1024 * 1024, File::create(out)?);
    writer.write_all(&ftyp)?;
    writer.write_all(&moov)?;
    writer.write_all(&1u32.to_be_bytes())?;
    writer.write_all(b"mdat")?;
    writer.write_all(&(media_len + 16).to_be_bytes())?;
    for chunk in &order {
        // Consecutive samples are usually next to each other in the input:
        // copy them as one range.
        let samples = &inputs[chunk.track].samples[chunk.first..chunk.first + chunk.count];
        let mut i = 0;
        while i < samples.len() {
            let start = samples[i].offset;
            let mut end = start + u64::from(samples[i].size);
            i += 1;
            while i < samples.len() && samples[i].offset == end {
                end += u64::from(samples[i].size);
                i += 1;
            }
            copy_range(&mut inputs[chunk.track].file, start, end - start, &mut writer)?;
        }
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn full(version: u8, flags: u32, rest: &[u8]) -> Vec<u8> {
        let mut body = vec![version, (flags >> 16) as u8, (flags >> 8) as u8, flags as u8];
        body.extend_from_slice(rest);
        body
    }

    fn u32s(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A tiny fragmented MP4 with one track: `fragments` fragments of
    /// `per_fragment` samples, each lasting `duration` and `size` bytes long,
    /// filled with `fill` plus the sample's number. Every `key`th sample is a
    /// keyframe (`key` 1: all are).
    pub(crate) fn fragmented(timescale: u32, fragments: u32, per_fragment: u32, duration: u32, size: u32, key: u32, fill: u8) -> Vec<u8> {
        let mut file = make_box(b"ftyp", b"dash\0\0\0\0iso6mp41");
        let mvhd = full(
            0,
            0,
            &[
                u32s(&[0, 0, timescale, 0, 0x0001_0000]),
                vec![1, 0, 0, 0],
                vec![0; 8],
                vec![0; 36],
                vec![0; 24],
                u32s(&[2]),
            ]
            .concat(),
        );
        let tkhd = full(0, 3, &[u32s(&[0, 0, 1, 0, 0]), vec![0; 60]].concat());
        let mdhd = full(0, 0, &[u32s(&[0, 0, timescale, 0]), vec![0x55, 0xc4, 0, 0]].concat());
        let stbl = make_box(
            b"stbl",
            &[
                make_box(b"stsd", &full(0, 0, &u32s(&[0]))),
                make_box(b"stts", &full(0, 0, &u32s(&[0]))),
                make_box(b"stsc", &full(0, 0, &u32s(&[0]))),
                make_box(b"stsz", &full(0, 0, &u32s(&[0, 0]))),
                make_box(b"stco", &full(0, 0, &u32s(&[0]))),
            ]
            .concat(),
        );
        let minf = make_box(b"minf", &stbl);
        let mdia = make_box(b"mdia", &[make_box(b"mdhd", &mdhd), minf].concat());
        let trak = make_box(b"trak", &[make_box(b"tkhd", &tkhd), mdia].concat());
        let trex = full(0, 0, &u32s(&[1, 1, 0, 0, 0]));
        file.extend_from_slice(&make_box(
            b"moov",
            &[make_box(b"mvhd", &mvhd), trak, make_box(b"mvex", &make_box(b"trex", &trex))].concat(),
        ));
        file.extend_from_slice(&make_box(b"sidx", &[0; 24]));
        let mut number = 0u32;
        for n in 0..fragments {
            let mut entries = Vec::new();
            let mut media = Vec::new();
            for _ in 0..per_fragment {
                let flags = if number.is_multiple_of(key) { 0 } else { NON_SYNC };
                entries.extend_from_slice(&u32s(&[duration, size, flags]));
                media.extend(std::iter::repeat_n(fill.wrapping_add(number as u8), size as usize));
                number += 1;
            }
            let tfhd = full(0, 0x020000, &u32s(&[1]));
            let tfdt = full(1, 0, &(u64::from(n * per_fragment * duration)).to_be_bytes());
            // data_offset is filled in once the moof's size is known.
            let trun_body = |offset: u32| full(0, 0x1 | 0x100 | 0x200 | 0x400, &[u32s(&[per_fragment, offset]), entries.clone()].concat());
            let build = |offset: u32| {
                let traf = make_box(
                    b"traf",
                    &[make_box(b"tfhd", &tfhd), make_box(b"tfdt", &tfdt), make_box(b"trun", &trun_body(offset))].concat(),
                );
                make_box(b"moof", &[make_box(b"mfhd", &full(0, 0, &u32s(&[n + 1]))), traf].concat())
            };
            let size_of_moof = build(0).len() as u32;
            file.extend_from_slice(&build(size_of_moof + 8));
            file.extend_from_slice(&make_box(b"mdat", &media));
        }
        file
    }

    struct Table<'a> {
        moov: &'a [u8],
        trak: usize,
    }

    impl Table<'_> {
        fn stbl(&self) -> &[u8] {
            let traks: Vec<Child> = children(self.moov).unwrap().into_iter().filter(|c| &c.kind == b"trak").collect();
            let trak = &self.moov[traks[self.trak].body..traks[self.trak].end];
            find(trak, &[b"mdia", b"minf", b"stbl"]).unwrap()
        }
        fn get(&self, kind: &[u8; 4]) -> Option<&[u8]> {
            find(self.stbl(), &[kind])
        }
    }

    #[test]
    fn joins_into_a_regular_mp4_with_complete_tables() {
        let dir = tempfile::tempdir().unwrap();
        // Video: 6 fragments of 5 frames (1/30 s each, every 5th a keyframe); audio: 2 fragments of 16 packets.
        std::fs::write(dir.path().join("v"), fragmented(90_000, 6, 5, 3000, 100, 5, 10)).unwrap();
        std::fs::write(dir.path().join("a"), fragmented(44_100, 2, 16, 1024, 50, 1, 200)).unwrap();
        let out = dir.path().join("joined.mp4");
        join(&dir.path().join("v"), &dir.path().join("a"), &out).unwrap();
        let data = std::fs::read(&out).unwrap();

        let top: Vec<[u8; 4]> = children(&data).unwrap().iter().map(|c| c.kind).collect();
        assert_eq!(top, [*b"ftyp", *b"moov", *b"mdat"], "a regular MP4: header, then the media");
        assert_eq!(&data[8..12], b"isom");
        let moov = find(&data, &[b"moov"]).unwrap();
        assert!(find(moov, &[b"mvex"]).is_none(), "no fragment defaults in a regular MP4");

        let video = Table { moov, trak: 0 };
        let audio = Table { moov, trak: 1 };
        // Sizes: 30 video frames of 100 bytes, 32 audio packets of 50.
        let stsz = video.get(b"stsz").unwrap();
        assert_eq!(u32_at(stsz, 8).unwrap(), 30);
        assert_eq!(u32_at(audio.get(b"stsz").unwrap(), 8).unwrap(), 32);
        // Timing: one run of 30 x 3000.
        let stts = video.get(b"stts").unwrap();
        assert_eq!(
            (u32_at(stts, 4).unwrap(), u32_at(stts, 8).unwrap(), u32_at(stts, 12).unwrap()),
            (1, 30, 3000)
        );
        // Keyframes 1, 6, 11, ... 26; audio has no table (all are).
        let stss = video.get(b"stss").unwrap();
        assert_eq!(
            (u32_at(stss, 4).unwrap(), u32_at(stss, 8).unwrap(), u32_at(stss, 16).unwrap()),
            (6, 1, 11)
        );
        assert!(audio.get(b"stss").is_none());

        // Every sample's bytes are where its chunk table says.
        for (table, fill, size) in [(&video, 10u8, 100usize), (&audio, 200u8, 50usize)] {
            let co64 = table.get(b"co64").unwrap();
            let stsc = table.get(b"stsc").unwrap();
            let chunks = u32_at(co64, 4).unwrap() as usize;
            let per_chunk = u32_at(stsc, 12).unwrap() as usize;
            let first = u64_at(co64, 8).unwrap() as usize;
            assert!(data[first..first + size].iter().all(|&b| b == fill), "first sample at its offset");
            let second_chunk = u64_at(co64, 16).unwrap() as usize;
            let expected = fill.wrapping_add(per_chunk as u8);
            assert!(chunks > 1 && data[second_chunk..second_chunk + size].iter().all(|&b| b == expected));
        }

        // Durations: 1 s of video (30 x 3000 / 90000) and about 0.74 s of sound.
        let mdhd = find(moov, &[b"trak", b"mdia", b"mdhd"]).unwrap();
        assert_eq!(u32_at(mdhd, 16).unwrap(), 90_000);
        let mdat_len = children(&data)
            .unwrap()
            .into_iter()
            .find(|c| &c.kind == b"mdat")
            .map(|c| c.end - c.body)
            .unwrap();
        assert_eq!(mdat_len, 30 * 100 + 32 * 50, "all media, nothing else");
    }
}
