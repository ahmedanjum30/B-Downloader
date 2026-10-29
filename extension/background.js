// Hands downloads to BDownloader through its native messaging host.
//
// A download the browser starts is cancelled before the browser shows it,
// so it never holds a connection that the server counts against
// BDownloader, and is offered to the app. If the app can't be reached, the
// browser downloads it again itself, so nothing is lost.

// Chromium browsers run this as a service worker that loads the rest itself;
// Firefox loads all three from its manifest, in order.
if (typeof importScripts === "function") importScripts("videos.js", "yt-capture.js");

const HOST = "com.bdownloader.host";
const MENU_ID = "bdownloader";

function isWebUrl(url) {
  return /^https?:\/\//i.test(url || "");
}

// A torrent's magnet link.
function isMagnet(url) {
  return /^magnet:\?xt=urn:btih:/i.test(url || "");
}

// Text and data files are small and often just opened; the browser keeps those.
const BROWSER_TYPES = new Set([
  "txt", "json", "xml", "csv", "tsv", "html", "htm", "js", "css", "md", "log",
  "ini", "cfg", "conf", "yaml", "yml", "ics", "vcf", "srt", "vtt", "svg", "rss",
]);

function keepInBrowser(item, url) {
  const name = (item.filename || "").split(/[\\/]/).pop() || (() => {
    try {
      return new URL(url).pathname.split("/").pop();
    } catch {
      return "";
    }
  })();
  const ext = name.includes(".") ? name.split(".").pop().toLowerCase() : "";
  if (ext) return BROWSER_TYPES.has(ext);
  // No extension: go by the type the server gave.
  const mime = (item.mime || "").toLowerCase();
  return /^text\//.test(mime) || /^application\/(json|xml|[\w.+-]*\+(json|xml))\b/.test(mime);
}

// Resolves to true if the app accepted the message.
function send(message) {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(HOST, message, (reply) => {
      resolve(!chrome.runtime.lastError && Boolean(reply && reply.ok));
    });
  });
}

// Which cookie store a download or tab uses. Incognito and private windows
// (and Firefox containers) keep their cookies apart from the normal ones, and
// a login made there only exists in their store. `from`: a tab (its id and,
// in Firefox, `cookieStoreId`) or a download (`incognito`, `cookieStoreId`).
async function cookieStore(from) {
  if (!from) return undefined;
  if (from.cookieStoreId) return from.cookieStoreId;
  try {
    const stores = await chrome.cookies.getAllCookieStores();
    if (from.tabId !== undefined && from.tabId >= 0) {
      const store = stores.find((s) => s.tabIds.includes(from.tabId));
      if (store) return store.id;
    }
    // Chrome names its Incognito store "1"; Firefox marks its private one.
    if (from.incognito) {
      const store = stores.find((s) => s.incognito) || stores.find((s) => s.id === "1");
      if (store) return store.id;
    }
  } catch {
    // Unknown: the normal store.
  }
  return undefined;
}

// The browser's cookies for the URL, so downloads that need a login work.
async function cookieHeader(url, from) {
  try {
    const storeId = await cookieStore(from);
    const cookies = await chrome.cookies.getAll(storeId ? { url, storeId } : { url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ") || null;
  } catch {
    return null;
  }
}

// `expect`: what the browser's own download is getting. BDownloader then
// checks the link first and only accepts it if it gets the same file.
// `from`: the tab or download it comes from, for its cookies.
async function handOver(url, referrer, fileName, origin, dir, expect, from) {
  return send({
    type: "add",
    url,
    referrer: referrer || null,
    file_name: fileName || null,
    cookies: await cookieHeader(url, from),
    user_agent: navigator.userAgent,
    origin: origin || null,
    dir: dir || null,
    expect: expect || null,
  });
}

function hello() {
  send({ type: "hello", browser: navigator.userAgent });
}

// Runs while the browser is still choosing the file name, before it shows
// the download anywhere or asks where to save it. The download waits here
// while BDownloader checks the link; if BDownloader takes it, it's cancelled
// and never appears.
if (chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    // Our own downloads stay in the browser.
    if (item.byExtensionId === chrome.runtime.id) return false;
    const url = item.finalUrl || item.url;
    // blob:, data: and file: downloads only exist inside the browser.
    if (!isWebUrl(url) || keepInBrowser(item, url)) return false;

    takeOver(item, url, suggest);
    return true; // suggest() is called once BDownloader has answered
  });
} else {
  // Firefox can't hold a download before it starts: it runs while
  // BDownloader checks the link, and is cancelled if BDownloader takes it.
  chrome.downloads.onCreated.addListener((item) => {
    if (item.byExtensionId === chrome.runtime.id) return;
    const url = item.finalUrl || item.url;
    if (!isWebUrl(url) || keepInBrowser(item, url)) return;
    takeOver(item, url, () => {}, true);
  });
}

async function takeOver(item, url, suggest, saved) {
  // By now the browser knows the real file name.
  const name = item.filename ? item.filename.split(/[\\/]/).pop() : null;
  // Firefox has already asked where to save it (when set to): keep that folder.
  const dir = saved && /[\\/]/.test(item.filename || "") ? item.filename.replace(/[\\/][^\\/]*$/, "") : null;
  // Some links work only once, or only for the browser (sites that unlock a
  // download after a check). So the browser's download stays until
  // BDownloader has checked it gets the same file.
  const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize > 0 ? item.fileSize : null;
  const expect = { mime: item.mime || null, size };
  const from = { incognito: item.incognito, cookieStoreId: item.cookieStoreId };
  const taken = await handOver(url, item.referrer, name, null, dir, expect, from);
  if (taken) {
    // Cancelled rather than paused: a paused download can keep its
    // connection open, and sites that limit connections count it against BDownloader.
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});
    // Gone from the browser, so there's nothing to name: suggest() would
    // only log "Invalid downloadId".
    return;
  }
  // BDownloader didn't take it (or isn't available): the browser carries on.
  suggest();
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: "Download with BDownloader",
      contexts: ["link", "image", "video", "audio"],
    });
  });
  hello();
});

chrome.runtime.onStartup.addListener(hello);

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  const url = info.linkUrl || info.srcUrl;
  const page = info.pageUrl || (tab && tab.url);
  if (isWebUrl(url) || isMagnet(url)) {
    handOver(url, page, null, null, null, null, tab && { tabId: tab.id, cookieStoreId: tab.cookieStoreId });
    return;
  }
  // Players that stream (blob: sources) have no link of their own; use the
  // video found in the tab's traffic instead.
  if (info.mediaType === "video" && tab) {
    const [video] = await videosFor(tab.id);
    if (video) handOver(video.url, video.referer || page, await videoName(tab.id, video), video.origin, null, null, { tabId: tab.id, cookieStoreId: tab.cookieStoreId });
  }
});

// From the popup: the tab's videos, and sending one to the app.
chrome.runtime.onMessage.addListener((message, sender, reply) => {
  if (message.type === "status") {
    load(message.tabId).then(
      (state) => reply({ chunks: Boolean(state.chunks), inline: Boolean(state.inline) }),
      () => reply({}),
    );
    return true;
  }
  if (message.type === "videos") {
    videosFor(message.tabId).then(reply, () => reply([]));
    return true;
  }
  // From content.js, on the page.
  const tabId = sender.tab && sender.tab.id;
  // A magnet link clicked on the page: BDownloader asks which files to get.
  if (message.type === "magnet" && isMagnet(message.url)) {
    handOver(message.url, sender.tab && sender.tab.url, null, null, null, null, null).then(reply, () => reply(false));
    return true;
  }
  if (message.type === "playlist" && tabId !== undefined) {
    addPlaylist(tabId, message.url, { referer: message.referer, origin: message.origin }).catch(() => {});
    return false;
  }
  if (message.type === "chunk" && tabId !== undefined) {
    dropChunks(tabId, message.url).catch(() => {});
    return false;
  }
  if (message.type === "inline-playlist" && tabId !== undefined) {
    update(tabId, (state) => {
      if (state.inline) return false;
      state.inline = true;
    }).catch(() => {});
    return false;
  }
  if (message.type === "videos-count" && tabId !== undefined) {
    videosFor(tabId).then((v) => reply(v.length), () => reply(0));
    return true;
  }
  if (message.type === "videos-here" && tabId !== undefined) {
    Promise.all([videosFor(tabId), pageTitle(tabId)]).then(([videos, title]) => reply({ videos, title }), () => reply({ videos: [] }));
    return true;
  }
  if (message.type === "download-from-page" && tabId !== undefined) {
    (async () => {
      // Only videos the extension found itself, whatever the page asks for.
      const video = (await videosFor(tabId)).find((v) => v.url === (message.video && message.video.url));
      if (!video) return reply(false);
      const from = { tabId, cookieStoreId: sender.tab.cookieStoreId };
      reply(await handOver(video.url, video.referer || sender.tab.url, await videoName(tabId, video), video.origin, null, null, from));
    })().catch(() => reply(false));
    return true;
  }
  // YouTube: whether the player's playback was captured, the qualities to
  // offer for the video the page describes, and a choice made.
  if (message.type === "youtube-ready" && tabId !== undefined) {
    youtubeReady(tabId, message.cpn).then(reply, () => reply(false));
    return true;
  }
  if (message.type === "youtube-choices") {
    reply(message.info && message.info.formats ? youtubeChoices(message.info) : []);
    return false;
  }
  if (message.type === "download-youtube" && tabId !== undefined) {
    sendYoutube(tabId, message.info, message.choice).then(reply, () => reply(false));
    return true;
  }
  if (message.type === "download" && message.video && message.video.kind === "youtube") {
    // From the popup: the best quality.
    (async () => {
      const info = await youtubeInfo(message.tabId);
      const choice = info && info.formats ? youtubeChoices(info)[0] : null;
      reply(choice ? await sendYoutube(message.tabId, info, choice) : false);
    })().catch(() => reply(false));
    return true;
  }
  if (message.type === "download") {
    (async () => {
      const tab = await chrome.tabs.get(message.tabId).catch(() => null);
      const name = await videoName(message.tabId, message.video);
      const from = tab && { tabId: tab.id, cookieStoreId: tab.cookieStoreId };
      reply(await handOver(message.video.url, message.video.referer || (tab && tab.url), name, message.video.origin, null, null, from));
    })().catch(() => reply(false));
    return true;
  }
  return false;
});
