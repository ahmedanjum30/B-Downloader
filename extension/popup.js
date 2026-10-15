// Lists the videos found on the current tab, each with a Download button.

const list = document.getElementById("list");

function size(bytes) {
  if (!bytes) return null;
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toFixed(i >= 2 ? 1 : 0)} ${units[i]}`;
}

// "28 min 47 sec"
function duration(seconds) {
  if (!seconds) return null;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h) return `${h} hr ${m} min`;
  return m ? `${m} min ${s} sec` : `${s} sec`;
}

function describe(video) {
  if (video.kind === "youtube") return "YouTube video · best quality (choose others on the video)";
  if (video.kind === "stream") {
    const kind = video.format ? `${video.format} file` : "Video stream";
    return [kind, video.quality, duration(video.duration)].filter(Boolean).join(" · ");
  }
  return [`${video.ext.toUpperCase()} file`, size(video.size)].filter(Boolean).join(" · ");
}

function fileName(url) {
  try {
    const last = new URL(url).pathname.split("/").filter(Boolean).pop() || url;
    return decodeURIComponent(last);
  } catch {
    return url;
  }
}

function row(tabId, title, video) {
  const item = document.createElement("div");
  item.className = "video";
  const info = document.createElement("div");
  info.className = "info";
  const name = document.createElement("div");
  name.className = "name";
  name.textContent = title || fileName(video.url);
  name.title = video.url;
  const meta = document.createElement("div");
  meta.className = "meta";
  meta.textContent = describe(video);
  info.append(name, meta);

  const button = document.createElement("button");
  button.textContent = "Download";
  button.addEventListener("click", async () => {
    button.disabled = true;
    const ok = await chrome.runtime.sendMessage({ type: "download", tabId, video }).catch(() => false);
    const status = document.createElement("span");
    status.className = ok ? "status ok" : "status bad";
    status.textContent = ok ? "Sent" : "Failed";
    status.title = ok ? "Confirm it in BDownloader" : "BDownloader isn't installed, or couldn't be started";
    button.replaceWith(status);
  });
  item.append(info, button);
  return item;
}

function note(text) {
  const empty = document.createElement("div");
  empty.className = "empty";
  empty.textContent = text;
  list.append(empty);
}

async function show() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  let videos;
  try {
    videos = await chrome.runtime.sendMessage({ type: "videos", tabId: tab.id });
  } catch {
    // The extension's background part isn't running this version yet.
    note("BDownloader's extension needs a restart: open chrome://extensions and click the reload button on BDownloader.");
    return;
  }
  if (!videos || videos.length === 0) {
    const status = await chrome.runtime.sendMessage({ type: "status", tabId: tab.id }).catch(() => ({}));
    if (status && status.inline) {
      note("This player builds its video list inside the page, so there's no link to download yet.");
    } else if (status && status.chunks) {
      note("This page plays a stream, but its playlist hasn't been seen. Reload the page, start the video, then open this again.");
    } else {
      note("No videos found yet. Start playing the video, then open this again.");
    }
    return;
  }
  for (const video of videos) list.append(row(tab.id, tab.title, video));
}

show();
