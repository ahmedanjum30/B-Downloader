// YouTube downloads. YouTube's player streams over its own protocol (SABR)
// with requests that carry a token proving they come from a real browser.
// This keeps the latest such request of each tab, so BDownloader can repeat
// it with the position moved on and download the whole video. The token
// arrives a few seconds into playback; earlier requests carry a stand-in
// that YouTube only accepts for a minute, so those aren't kept.

const YT_TOKEN_MIN_BYTES = 30;
const YOUTUBE_PAGE = /^https:\/\/(www\.|m\.)?youtube\.com\//;
// YouTube's own pages and the players other sites embed.
const YOUTUBE_FRAME = /^https:\/\/(www\.|m\.)?youtube(-nocookie)?\.com\//;

function ytVarint(bytes, pos) {
  let value = 0;
  let shift = 0;
  for (;;) {
    if (pos >= bytes.length) return [-1, pos];
    const b = bytes[pos++];
    value += (b & 0x7f) * 2 ** shift;
    shift += 7;
    if (!(b & 0x80)) return [value, pos];
  }
}

// The bytes of field `number` at the top level of a protobuf message.
function ytField(bytes, number) {
  let pos = 0;
  while (pos < bytes.length) {
    let key;
    [key, pos] = ytVarint(bytes, pos);
    if (key < 0) return null;
    const wire = key % 8;
    const field = Math.floor(key / 8);
    if (wire === 0) [, pos] = ytVarint(bytes, pos);
    else if (wire === 2) {
      let len;
      [len, pos] = ytVarint(bytes, pos);
      if (len < 0 || pos + len > bytes.length) return null;
      if (field === number) return bytes.subarray(pos, pos + len);
      pos += len;
    } else if (wire === 5) pos += 4;
    else if (wire === 1) pos += 8;
    else return null;
  }
  return null;
}

function ytBase64(bytes) {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

// YouTube's player sends these from a worker, which the browser doesn't
// count as part of any tab. So captures are kept by the playback's ID
// ("cpn" in the address), which the page's player also reports; the tab's
// own script asks for the one matching its player.
const YT_MAX_CAPTURES = 20;

function captureKey(cpn) {
  return `ytcap:${cpn}`;
}

chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (details.method !== "POST" || !/[?&]sabr=1/.test(details.url)) return;
    let cpn;
    try {
      cpn = new URL(details.url).searchParams.get("cpn");
    } catch {
      return;
    }
    const raw = details.requestBody && details.requestBody.raw;
    if (!cpn || !raw || !raw[0] || !raw[0].bytes) return;
    const body = new Uint8Array(raw[0].bytes);
    const context = ytField(body, 19);
    const token = context && ytField(context, 2);
    if (!token || token.length < YT_TOKEN_MIN_BYTES) return;
    // The newest request carries the freshest token and address.
    const capture = { url: details.url, request: ytBase64(body), at: Date.now() };
    chrome.storage.session.set({ [captureKey(cpn)]: capture }).then(forgetOldCaptures, () => {});
  },
  { urls: ["*://*.googlevideo.com/videoplayback*"] },
  ["requestBody"],
);

async function forgetOldCaptures() {
  const all = await chrome.storage.session.get(null);
  const captures = Object.entries(all)
    .filter(([key]) => key.startsWith("ytcap:"))
    .sort((a, b) => b[1].at - a[1].at);
  const old = captures.slice(YT_MAX_CAPTURES).map(([key]) => key);
  if (old.length) await chrome.storage.session.remove(old);
}

async function youtubeCapture(cpn) {
  if (!cpn) return null;
  const key = captureKey(cpn);
  return (await chrome.storage.session.get(key))[key] || null;
}

// From a YouTube player's frame: is its playback captured? Marks the tab, so
// the badge and the bar show it.
async function youtubeReady(tabId, cpn) {
  const ready = Boolean(await youtubeCapture(cpn));
  await update(tabId, (state) => {
    const current = state.youtube && state.youtube.cpn;
    if (ready && current !== cpn) state.youtube = { cpn, at: Date.now() };
    else if (!ready && current) delete state.youtube;
    else return false;
  });
  return ready;
}

// Moving to another video (YouTube changes pages without loading them)
// makes the capture stale.
chrome.webNavigation.onHistoryStateUpdated.addListener((details) => {
  if (details.frameId !== 0 || !YOUTUBE_PAGE.test(details.url)) return;
  update(details.tabId, (state) => {
    if (!state.youtube) return false;
    delete state.youtube;
  }).catch(() => {});
});

// Asks the tab for the video's title and formats: the YouTube page itself,
// or a YouTube player embedded in another site's page.
async function youtubeInfo(tabId) {
  const frames = (await chrome.webNavigation.getAllFrames({ tabId }).catch(() => null)) || [];
  for (const frame of frames.filter((f) => YOUTUBE_FRAME.test(f.url))) {
    const info = await chrome.tabs.sendMessage(tabId, { type: "youtube-info" }, { frameId: frame.frameId }).catch(() => null);
    if (info) return info;
  }
  return null;
}

// The formats for a choice: the picture at `height` (H.264 where there is
// one, else AV1), and the best AAC sound in the video's own language.
function youtubeFormats(info, height) {
  const audio = info.formats
    .filter((f) => f.mimeType.startsWith("audio/mp4") && /mp4a/.test(f.mimeType))
    .sort((a, b) => Number(b.defaultAudio) - Number(a.defaultAudio) || Number(Boolean(a.xtags)) - Number(Boolean(b.xtags)) || b.bitrate - a.bitrate)[0];
  if (height === 0) return { video: null, audio };
  const candidates = info.formats.filter((f) => f.mimeType.startsWith("video/mp4") && f.height === height);
  const video =
    candidates.filter((f) => /avc1/.test(f.mimeType)).sort((a, b) => b.bitrate - a.bitrate)[0] ||
    candidates.sort((a, b) => b.bitrate - a.bitrate)[0];
  return { video, audio };
}

// The qualities to offer, best first: one per height, then sound only.
function youtubeChoices(info) {
  const heights = [...new Set(info.formats.filter((f) => f.mimeType.startsWith("video/mp4") && f.height).map((f) => f.height))].sort((a, b) => b - a);
  const choices = heights.map((height) => ({ height, ...youtubeFormats(info, height) })).filter((c) => c.video && c.audio);
  const sound = youtubeFormats(info, 0);
  if (sound.audio) choices.push({ height: 0, ...sound });
  return choices;
}

const ytFormat = (f) =>
  f && {
    itag: f.itag,
    last_modified: f.lastModified,
    xtags: f.xtags,
    mime_type: f.mimeType,
    content_length: f.contentLength,
  };

// Hands a YouTube video to the app: the captured request plus the choice.
async function sendYoutube(tabId, info, choice) {
  // The capture of this very playback, so another tab or an earlier video can't be mixed in.
  const capture = await youtubeCapture(info && info.cpn);
  if (!capture || !choice || !choice.audio) return false;
  const watch = `https://www.youtube.com/watch?v=${encodeURIComponent(info.videoId)}`;
  const title = info.title ? info.title.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 150) : "";
  // The quality in the name, so downloads of one video in several qualities can be told apart.
  const quality = choice.video ? choice.video.qualityLabel || `${choice.height}p` : null;
  const name = title && quality ? `${title} (${quality})` : title || null;
  return send({
    type: "add",
    url: watch,
    file_name: name,
    referrer: watch,
    youtube: {
      video_id: info.videoId,
      url: capture.url,
      request: capture.request,
      video: ytFormat(choice.video),
      audio: ytFormat(choice.audio),
      duration_ms: info.lengthSeconds ? info.lengthSeconds * 1000 : null,
    },
  });
}
