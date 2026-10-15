// Hands downloads to BDownloader through its native messaging host.
//
// A download the browser starts is cancelled before the browser shows it,
// so it never holds a connection that the server counts against
// BDownloader, and is offered to the app. If the app can't be reached, the
// browser downloads it again itself, so nothing is lost.

importScripts("videos.js", "yt-capture.js");

const HOST = "com.bdownloader.host";
const MENU_ID = "bdownloader";

function isWebUrl(url) {
  return /^https?:\/\//i.test(url || "");
}

// Resolves to true if the app accepted the message.
function send(message) {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(HOST, message, (reply) => {
      resolve(!chrome.runtime.lastError && Boolean(reply && reply.ok));
    });
  });
}

// The browser's cookies for the URL, so downloads that need a login work.
async function cookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ") || null;
  } catch {
    return null;
  }
}

async function handOver(url, referrer, fileName, origin) {
  return send({
    type: "add",
    url,
    referrer: referrer || null,
    file_name: fileName || null,
    cookies: await cookieHeader(url),
    user_agent: navigator.userAgent,
    origin: origin || null,
  });
}

function hello() {
  send({ type: "hello", browser: navigator.userAgent });
}

// Runs while the browser is still choosing the file name, before it shows
// the download anywhere or asks where to save it. Holding the download here
// and cancelling it means the browser's own download never appears.
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  // Our own fallback downloads (below) stay in the browser.
  if (item.byExtensionId === chrome.runtime.id) return false;
  const url = item.finalUrl || item.url;
  // blob:, data: and file: downloads only exist inside the browser.
  if (!isWebUrl(url)) return false;

  takeOver(item, url, suggest);
  return true; // suggest() is called once the download is cancelled
});

async function takeOver(item, url, suggest) {
  // Cancel rather than pause: a paused download can keep its connection
  // open, and sites that limit connections count it against BDownloader.
  await chrome.downloads.cancel(item.id).catch(() => {});
  await chrome.downloads.erase({ id: item.id }).catch(() => {});
  try {
    suggest();
  } catch {
    // Already cancelled; nothing left to release.
  }

  // By now the browser knows the real file name.
  const name = item.filename ? item.filename.split(/[\\/]/).pop() : null;
  if (!(await handOver(url, item.referrer, name))) {
    // BDownloader isn't available: let the browser download it after all.
    chrome.downloads.download({ url: item.url }).catch(() => {});
  }
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
  if (isWebUrl(url)) {
    handOver(url, page, null);
    return;
  }
  // Players that stream (blob: sources) have no link of their own; use the
  // video found in the tab's traffic instead.
  if (info.mediaType === "video" && tab) {
    const [video] = await videosFor(tab.id);
    if (video) handOver(video.url, video.referer || page, await videoName(tab.id, video), video.origin);
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
      reply(await handOver(video.url, video.referer || sender.tab.url, await videoName(tabId, video), video.origin));
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
      reply(await handOver(message.video.url, message.video.referer || (tab && tab.url), name, message.video.origin));
    })().catch(() => reply(false));
    return true;
  }
  return false;
});
