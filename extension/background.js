// Hands downloads to BDownloader through its native messaging host.
//
// Downloads the browser starts are paused at once and offered to the app.
// If the app takes them, the browser's copy is cancelled; if the app can't
// be reached, the browser just carries on.

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

async function handOver(url, referrer, fileName) {
  return send({
    type: "add",
    url,
    referrer: referrer || null,
    file_name: fileName || null,
    cookies: await cookieHeader(url),
    user_agent: navigator.userAgent,
  });
}

function hello() {
  send({ type: "hello", browser: navigator.userAgent });
}

chrome.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url;
  // blob:, data: and file: downloads only exist inside the browser.
  if (!isWebUrl(url) || item.state !== "in_progress") return;

  await chrome.downloads.pause(item.id).catch(() => {});
  const name = item.filename ? item.filename.split(/[\\/]/).pop() : null;
  if (await handOver(url, item.referrer, name)) {
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});
  } else {
    await chrome.downloads.resume(item.id).catch(() => {});
  }
});

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

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  const url = info.linkUrl || info.srcUrl;
  if (isWebUrl(url)) handOver(url, info.pageUrl || (tab && tab.url), null);
});
