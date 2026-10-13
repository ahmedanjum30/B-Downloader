// Hands downloads to BDownloader through its native messaging host.
//
// A download the browser starts is cancelled before the browser shows it,
// so it never holds a connection that the server counts against
// BDownloader, and is offered to the app. If the app can't be reached, the
// browser downloads it again itself, so nothing is lost.

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

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  const url = info.linkUrl || info.srcUrl;
  if (isWebUrl(url)) handOver(url, info.pageUrl || (tab && tab.url), null);
});
