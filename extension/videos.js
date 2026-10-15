// Finds the videos a tab plays, so they can be sent to BDownloader.
//
// The extension watches the tab's network traffic for video files (MP4,
// WebM, ...) and HLS playlists (.m3u8). Playlists are read to tell a list of
// qualities from the playlist of one quality, so each video shows up once.
// The list is kept in session storage, because the browser stops this
// script when it's idle.

const VIDEO_FILE = /^video\/(mp4|webm|x-matroska|quicktime|x-flv|x-msvideo|3gpp|ogg)\b/i;
const PLAYLIST = /mpegurl/i;
// Parts of streams, which make no sense alone.
const STREAM_PART = /\.(ts|m4s|m4v|m4a|aac)(\?|$)/i;
// YouTube can't be downloaded this way, and the Chrome Web Store doesn't
// allow extensions that try.
const BLOCKED_HOSTS = /(^|\.)(youtube\.com|googlevideo\.com|youtube-nocookie\.com)$/i;
// Smaller video files are mostly previews and ads.
const MIN_FILE_SIZE = 512 * 1024;
const MAX_PER_TAB = 50;

const FILE_EXTENSIONS = {
  mp4: "mp4", webm: "webm", "x-matroska": "mkv", quicktime: "mov",
  "x-flv": "flv", "x-msvideo": "avi", "3gpp": "3gp", ogg: "ogv",
};

function header(details, name) {
  const h = (details.responseHeaders || []).find((h) => h.name.toLowerCase() === name);
  return h ? h.value || "" : "";
}

// The full size of the file, also when the player asked for a part of it.
function fileSize(details) {
  const total = /\/(\d+)\s*$/.exec(header(details, "content-range"));
  if (total) return Number(total[1]);
  const length = Number(header(details, "content-length"));
  return details.statusCode === 200 && length > 0 ? length : null;
}

async function load(tabId) {
  const key = `tab:${tabId}`;
  const stored = await chrome.storage.session.get(key);
  return stored[key] || { videos: [], hidden: [] };
}

async function save(tabId, state) {
  await chrome.storage.session.set({ [`tab:${tabId}`]: state });
  const count = state.videos.filter((v) => !state.hidden.includes(v.url)).length;
  chrome.action.setBadgeText({ tabId, text: count ? String(count) : "" }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#6c63ff" }).catch(() => {});
  // The bars on the page's videos appear once there's something to download.
  chrome.tabs.sendMessage(tabId, { type: "videos-count", count }).catch(() => {});
}

// One change at a time per tab, so two requests finishing together don't
// overwrite each other's additions.
const queues = new Map();
function update(tabId, change) {
  const run = (queues.get(tabId) || Promise.resolve()).then(async () => {
    const state = await load(tabId);
    if (change(state) !== false) await save(tabId, state);
  });
  queues.set(tabId, run.catch(() => {}));
  return run;
}

function addVideo(tabId, video) {
  return update(tabId, (state) => {
    if (state.videos.some((v) => v.url === video.url)) return false;
    // The page may have reported it as a stream chunk while it was being checked.
    if (video.kind === "file" && chunkHosts.has(new URL(video.url).hostname)) return false;
    state.videos.push(video);
    if (state.videos.length > MAX_PER_TAB) state.videos.shift();
  });
}

// Fetches as the page's player would: many video servers only answer
// requests with their player's Referer and Origin, which an extension can't
// set on a request itself. A short-lived rule adds them to the extension's
// own requests to that server. One at a time, so rules don't mix.
let playerFetches = Promise.resolve();
let nextRuleId = 1;

function fetchAsPlayer(url, source = {}, init = {}) {
  const run = playerFetches.then(async () => {
    const headers = [];
    if (source.referer) headers.push({ header: "referer", operation: "set", value: source.referer });
    if (source.origin) headers.push({ header: "origin", operation: "set", value: source.origin });
    const rules = chrome.declarativeNetRequest;
    if (!headers.length || !rules) return fetch(url, { credentials: "include", ...init });
    const id = nextRuleId++;
    if (nextRuleId > 100000) nextRuleId = 1;
    await rules.updateSessionRules({
      removeRuleIds: [id],
      addRules: [
        {
          id,
          priority: 1,
          action: { type: "modifyHeaders", requestHeaders: headers },
          condition: {
            requestDomains: [new URL(url).hostname],
            tabIds: [chrome.tabs.TAB_ID_NONE],
            resourceTypes: ["xmlhttprequest", "other"],
          },
        },
      ],
    });
    try {
      return await fetch(url, { credentials: "include", ...init });
    } finally {
      await rules.updateSessionRules({ removeRuleIds: [id] }).catch(() => {});
    }
  });
  playerFetches = run.catch(() => {});
  return run;
}

// Reads a playlist: a list of qualities hides its qualities' own playlists;
// a single quality gets its duration. Live streams aren't listed.
async function describePlaylist(url, source, nested = false) {
  let text;
  try {
    const response = await fetchAsPlayer(url, source);
    if (!response.ok) return null;
    text = await response.text();
  } catch {
    return null;
  }
  if (!text.trimStart().startsWith("#EXTM3U")) return null;
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  if (text.includes("#EXT-X-STREAM-INF")) {
    const children = [];
    let best = null;
    lines.forEach((line, i) => {
      if (!line.startsWith("#EXT-X-STREAM-INF")) return;
      const next = lines[i + 1];
      if (!next || next.startsWith("#")) return;
      const child = new URL(next, url).href;
      children.push(child);
      const height = Number((/RESOLUTION=\d+x(\d+)/i.exec(line) || [])[1] || 0);
      const bandwidth = Number((/[,:]BANDWIDTH=(\d+)/i.exec(line) || [])[1] || 0);
      if (!best || height > best.height || (height === best.height && bandwidth > best.bandwidth)) {
        best = { url: child, height, bandwidth };
      }
    });
    for (const line of lines) {
      const uri = /^#EXT-X-MEDIA:.*URI="([^"]+)"/i.exec(line);
      if (uri) children.push(new URL(uri[1], url).href);
    }
    // The best quality's own playlist tells the length and the format.
    const media = best && !nested ? await describePlaylist(best.url, source, true) : null;
    if (best && !nested && !media) return null; // live, or unreadable
    return {
      quality: best && best.height ? `${best.height}p` : null,
      duration: media ? media.duration : null,
      format: media ? media.format : null,
      children,
    };
  }
  if (!text.includes("#EXT-X-ENDLIST")) return null; // live
  const seconds = lines
    .filter((l) => l.startsWith("#EXTINF:"))
    .reduce((sum, l) => sum + (parseFloat(l.slice(8)) || 0), 0);
  // Fragmented-MP4 streams start with a header (EXT-X-MAP); the rest are MPEG-TS.
  return { duration: Math.round(seconds), format: text.includes("#EXT-X-MAP") ? "MP4" : "TS", children: [] };
}

// The Referer and Origin the player sent with each request. Video servers
// often only answer requests that come from their own player, which is
// usually a page inside the site (an iframe), not the page in the address bar.
const sentHeaders = new Map();
const REQUEST_TYPES = ["media", "xmlhttprequest", "other", "object"];

chrome.webRequest.onSendHeaders.addListener(
  (details) => {
    if (details.tabId < 0) return;
    const get = (name) => {
      const h = (details.requestHeaders || []).find((h) => h.name.toLowerCase() === name);
      return h ? h.value : null;
    };
    const sent = { referer: get("referer"), origin: get("origin") };
    if (!sent.referer && !sent.origin) return;
    sentHeaders.set(details.requestId, sent);
    if (sentHeaders.size > 500) sentHeaders.delete(sentHeaders.keys().next().value);
  },
  { urls: ["<all_urls>"], types: REQUEST_TYPES },
  ["requestHeaders", "extraHeaders"],
);

// Where a request came from: what the player sent, or else the frame that made it.
function requestSource(details) {
  const sent = sentHeaders.get(details.requestId) || {};
  sentHeaders.delete(details.requestId);
  const initiator = details.initiator && details.initiator !== "null" ? details.initiator : null;
  return {
    referer: sent.referer || (initiator ? `${initiator}/` : null),
    origin: sent.origin || null,
  };
}

// Adds a playlist found in the tab's traffic, or by hook.js inside the page.
async function addPlaylist(tabId, playlistUrl, source = {}) {
  try {
    if (BLOCKED_HOSTS.test(new URL(playlistUrl).hostname)) return;
  } catch {
    return;
  }
  const known = await load(tabId);
  if (known.videos.some((v) => v.url === playlistUrl) || known.hidden.includes(playlistUrl)) return;
  const info = await describePlaylist(playlistUrl, source);
  if (!info) return;
  await update(tabId, (state) => {
    for (const child of info.children) if (!state.hidden.includes(child)) state.hidden.push(child);
    if (state.videos.some((v) => v.url === playlistUrl)) return;
    state.videos.push({
      url: playlistUrl,
      kind: "stream",
      quality: info.quality || null,
      duration: info.duration || null,
      format: info.format || null,
      referer: source.referer || null,
      origin: source.origin || null,
      found: Date.now(),
    });
  });
}

// Servers seen sending stream chunks labelled as video files; their other
// "video files" are chunks too.
const chunkHosts = new Set();
// Tabs known to be playing a stream.
const streamingTabs = new Set();

// Responses that might be a playlist under a disguise (any name, a text or
// binary type), per tab. They're only read once the tab turns out to be
// playing a stream whose playlist wasn't found the usual way.
const candidates = new Map(); // tabId -> [{ url, source, checked }]
const MAX_CANDIDATES = 40;
const NOT_PLAYLIST = /^\s*(video|audio|font)\/|javascript|ecmascript|css|html|json|wasm|xml|svg|gif|webp|avif|icon/i;

function noteCandidate(details, type, source) {
  if (details.type !== "xmlhttprequest" && details.type !== "other") return;
  const length = Number(header(details, "content-length"));
  if (length > 2 * 1024 * 1024 || NOT_PLAYLIST.test(type)) return;
  const list = candidates.get(details.tabId) || [];
  if (list.some((c) => c.url === details.url)) return;
  list.push({ url: details.url, source, checked: false });
  if (list.length > MAX_CANDIDATES) list.shift();
  candidates.set(details.tabId, list);
  if (streamingTabs.has(details.tabId)) checkCandidates(details.tabId);
}

// Reads the tab's candidates, newest first, until a playlist turns up.
const checking = new Map();
function checkCandidates(tabId) {
  const run = (checking.get(tabId) || Promise.resolve()).then(async () => {
    for (const candidate of [...(candidates.get(tabId) || [])].reverse()) {
      if (candidate.checked) continue;
      candidate.checked = true;
      const state = await load(tabId);
      if (state.videos.some((v) => v.kind === "stream")) return;
      if (await startsLikePlaylist(candidate.url, candidate.source)) {
        await addPlaylist(tabId, candidate.url, candidate.source);
      }
    }
  });
  checking.set(tabId, run.catch(() => {}));
}

async function startsLikePlaylist(url, source) {
  try {
    const response = await fetchAsPlayer(url, source);
    if (!response.ok || !response.body) return false;
    const reader = response.body.getReader();
    const { value } = await reader.read();
    reader.cancel().catch(() => {});
    const text = value ? new TextDecoder().decode(value.subarray(0, 64)) : "";
    return text.replace(/^﻿/, "").trimStart().startsWith("#EXTM3U");
  } catch {
    return false;
  }
}

// A stream player fetches chunk after chunk from one server, each under its
// own address; a video file is one address. So a second, different small
// "video file" from the same server in a tab means its files are chunks.
// This also works when the player runs where the page script can't see it.
const CHUNK_MAX_SIZE = 20 * 1024 * 1024;
const smallVideos = new Map(); // "tab host" -> addresses seen

function looksLikeChunks(tabId, url, size) {
  if (size !== null && size > CHUNK_MAX_SIZE) return false;
  const key = `${tabId} ${url.hostname}`;
  const seen = smallVideos.get(key) || new Set();
  // The same file asked for again (in byte ranges) is still one address.
  seen.add(url.href);
  smallVideos.set(key, seen);
  if (smallVideos.size > 200) smallVideos.delete(smallVideos.keys().next().value);
  return seen.size >= 2;
}

// The page saw that a server's "video files" are chunks of a stream: forget
// the ones listed, and note that the tab plays a stream.
async function dropChunks(tabId, chunkUrl) {
  let host;
  try {
    host = new URL(chunkUrl).hostname;
  } catch {
    return;
  }
  if (chunkHosts.has(host) && streamingTabs.has(tabId)) return;
  chunkHosts.add(host);
  streamingTabs.add(tabId);
  // The playlist may be among what the tab already loaded, under a disguise.
  checkCandidates(tabId);
  await update(tabId, (state) => {
    const hostOf = (u) => {
      try {
        return new URL(u).hostname;
      } catch {
        return "";
      }
    };
    state.videos = state.videos.filter((v) => v.kind === "stream" || hostOf(v.url) !== host);
    state.chunks = true;
  });
}

// Whether the file starts like a complete video (MP4/MOV, WebM/MKV, FLV, AVI,
// Ogg) rather than a chunk of a stream (MPEG-TS, MP4 fragment). `null` when
// it couldn't be checked, e.g. the server refused the extension's request.
async function isWholeVideo(url, source) {
  try {
    const response = await fetchAsPlayer(url, source, { headers: { Range: "bytes=0-31" } });
    if (!response.ok || !response.body) return null;
    const reader = response.body.getReader();
    const { value } = await reader.read();
    reader.cancel().catch(() => {});
    if (!value || value.length < 12) return null;
    const text = (from, to) => String.fromCharCode(...value.slice(from, to));
    if (text(4, 8) === "ftyp") return true;
    if (value[0] === 0x1a && value[1] === 0x45 && value[2] === 0xdf && value[3] === 0xa3) return true;
    return ["FLV", "RIFF", "OggS"].some((magic) => text(0, magic.length) === magic);
  } catch {
    return null;
  }
}

async function onHeaders(details) {
  if (details.tabId < 0 || details.statusCode < 200 || details.statusCode >= 300) return;
  let url;
  try {
    url = new URL(details.url);
  } catch {
    return;
  }
  if (!/^https?:$/.test(url.protocol) || BLOCKED_HOSTS.test(url.hostname)) return;
  const type = header(details, "content-type").toLowerCase();

  const source = requestSource(details);
  if (PLAYLIST.test(type) || /\.m3u8$/i.test(url.pathname)) {
    await addPlaylist(details.tabId, details.url, source);
    return;
  }

  // Chunks that say what they are.
  if (/^video\/(mp2t|iso\.segment)/.test(type) || (/^(video|audio)\//.test(type) && STREAM_PART.test(url.pathname))) {
    await dropChunks(details.tabId, details.url);
    return;
  }
  const file = VIDEO_FILE.exec(type);
  if (!file) {
    noteCandidate(details, type, source);
    return;
  }
  const size = fileSize(details);
  if (chunkHosts.has(url.hostname) || looksLikeChunks(details.tabId, url, size)) {
    await dropChunks(details.tabId, details.url);
    return;
  }
  if (size !== null && size < MIN_FILE_SIZE) return;
  const known = await load(details.tabId);
  if (known.videos.some((v) => v.url === details.url)) return;
  // Streams are often served in chunks labelled as video files.
  if ((await isWholeVideo(details.url, source)) === false) {
    await dropChunks(details.tabId, details.url);
    return;
  }
  await addVideo(details.tabId, {
    url: details.url,
    kind: "file",
    ext: FILE_EXTENSIONS[file[1].toLowerCase()] || "mp4",
    size,
    referer: source.referer,
    origin: source.origin,
    found: Date.now(),
  });
}

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    onHeaders(details).catch(() => {});
  },
  { urls: ["<all_urls>"], types: REQUEST_TYPES },
  ["responseHeaders"],
);

// A new page in the tab starts with an empty list.
chrome.webNavigation.onCommitted.addListener((details) => {
  if (details.frameId !== 0) return;
  for (const key of smallVideos.keys()) if (key.startsWith(`${details.tabId} `)) smallVideos.delete(key);
  candidates.delete(details.tabId);
  streamingTabs.delete(details.tabId);
  update(details.tabId, (state) => {
    state.videos = [];
    state.hidden = [];
    state.chunks = false;
    state.inline = false;
  });
});

chrome.tabs.onRemoved.addListener((tabId) => {
  chrome.storage.session.remove(`tab:${tabId}`).catch(() => {});
  candidates.delete(tabId);
  streamingTabs.delete(tabId);
});

// The videos to show for a tab: streams first (newest first), as they're
// usually the whole programme, then files from the largest.
async function videosFor(tabId) {
  const state = await load(tabId);
  const shown = state.videos.filter((v) => !state.hidden.includes(v.url));
  const streams = shown.filter((v) => v.kind === "stream").reverse();
  const files = shown.filter((v) => v.kind !== "stream").sort((a, b) => (b.size || 0) - (a.size || 0));
  return [...streams, ...files];
}

// The page's title, cleaned up to serve as a file name.
async function pageTitle(tabId) {
  let title = "";
  try {
    title = (await chrome.tabs.get(tabId)).title || "";
  } catch {
    // The tab is gone; the app names it after the link.
  }
  return title.replace(/[\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 150);
}

// The page's title, as the video's name.
async function videoName(tabId, video) {
  const title = await pageTitle(tabId);
  if (!title) return null;
  return video.kind === "file" ? `${title}.${video.ext}` : title;
}
