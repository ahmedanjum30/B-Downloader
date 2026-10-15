// Shows a "Download this video" bar on the page's video players, like IDM,
// once the extension has found a video in the tab. Also passes on the
// playlists hook.js spots inside the page.

(() => {
  // A video smaller than this is a preview or an ad.
  const MIN_WIDTH = 240;
  const MIN_HEIGHT = 135;

  // After the extension is reloaded or updated, this script keeps running in
  // pages that were already open, but can no longer reach the extension:
  // its calls throw at once instead of failing like other errors.
  function connected() {
    try {
      return Boolean(chrome.runtime && chrome.runtime.id);
    } catch {
      return false;
    }
  }

  function ask(message) {
    if (!connected()) return Promise.reject(new Error("BDownloader's extension was reloaded"));
    try {
      return chrome.runtime.sendMessage(message);
    } catch (e) {
      return Promise.reject(e);
    }
  }

  // Playlists found by hook.js, which can't talk to the extension itself.
  window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data) return;
    const kind = event.data.bdownloader;
    if (kind !== "playlist" && kind !== "chunk" && kind !== "inline-playlist") return;
    // The player runs in this frame, so its requests carry this page as their Referer.
    ask({ type: kind, url: String(event.data.url), referer: location.href, origin: location.origin }).catch(() => {});
  });

  let count = 0;
  const dismissed = new WeakSet();
  let current = null; // the video the bar is on
  let host = null;
  let bar = null;
  let label = null;
  let menu = null;

  function build() {
    host = document.createElement("div");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; top: 0; left: 0; display: none;";
    const root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        * { box-sizing: border-box; font: 600 11px/14px "Segoe UI", system-ui, sans-serif; }
        .bar { display: flex; align-items: center; border-radius: 5px; overflow: hidden;
               background: rgba(24, 26, 34, .92); color: #fff; box-shadow: 0 2px 10px rgba(0,0,0,.35); }
        button { all: unset; cursor: pointer; padding: 2px 8px; display: flex; align-items: center; gap: 5px; color: #fff; }
        button:hover { background: rgba(255,255,255,.12); }
        .main { background: #6c63ff; }
        .main:hover { background: #5a52e6; }
        .close { padding: 2px 6px; color: rgba(255,255,255,.7); }
        svg { width: 12px; height: 12px; }
        .menu { position: absolute; right: 0; top: 100%; margin-top: 4px; padding: 4px 0; border-radius: 6px;
                background: rgba(32, 33, 36, .97); color: #fff; box-shadow: 0 4px 16px rgba(0,0,0,.45);
                display: none; width: max-content; max-width: min(560px, 90vw); }
        .menu button { display: block; width: 100%; box-sizing: border-box; padding: 6px 16px; font-weight: 400; font-size: 12.5px;
                       line-height: 18px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
        .menu .all { font-weight: 600; }
        .menu .heading { padding: 6px 16px 4px; font-size: 12.5px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 420px; }
        .menu hr { border: 0; border-top: 1px solid rgba(255,255,255,.15); margin: 4px 0; }
        .ok { background: #1f9d55 !important; }
        .bad { background: #c0392b !important; }
      </style>
      <div class="bar">
        <button class="main" title="Download with BDownloader">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>
          </svg>
          <span>Download this video</span>
        </button>
        <button class="close" title="Hide">&#x2715;</button>
      </div>
      <div class="menu"></div>`;
    bar = root.querySelector(".main");
    label = root.querySelector(".main span");
    menu = root.querySelector(".menu");
    bar.addEventListener("click", onDownloadClick);
    root.querySelector(".close").addEventListener("click", () => {
      if (current) dismissed.add(current);
      hide();
    });
    document.documentElement.appendChild(host);
  }

  function hide() {
    if (host) host.style.display = "none";
    if (menu) menu.style.display = "none";
    current = null;
  }

  // Every video in the page, including those inside players built as web
  // components (shadow DOM), which a plain search of the page doesn't reach.
  // Extensions may look into closed ones too.
  let videoCache = { at: 0, videos: [] };
  function videosInPage() {
    // Searching a big page takes a few milliseconds; once a second is plenty.
    if (Date.now() - videoCache.at > 1000) videoCache = { at: Date.now(), videos: allVideos() };
    return videoCache.videos;
  }

  function allVideos(root = document, found = []) {
    found.push(...root.querySelectorAll("video"));
    for (const el of root.querySelectorAll("*")) {
      let shadow = el.shadowRoot;
      if (!shadow) {
        try {
          // Chromium: chrome.dom; Firefox: a method on the element itself.
          if (chrome.dom && chrome.dom.openOrClosedShadowRoot) shadow = chrome.dom.openOrClosedShadowRoot(el);
          else if (typeof el.openOrClosedShadowRoot === "function") shadow = el.openOrClosedShadowRoot();
        } catch {
          shadow = null;
        }
      }
      if (shadow) allVideos(shadow, found);
    }
    return found;
  }

  // Whether `node` is inside `outer`, also through shadow DOM boundaries.
  function within(outer, node) {
    for (let n = node; n; n = n.parentNode || n.host) {
      if (n === outer) return true;
    }
    return false;
  }

  function usable(video) {
    if (dismissed.has(video) || !video.isConnected) return false;
    const r = video.getBoundingClientRect();
    return r.width >= MIN_WIDTH && r.height >= MIN_HEIGHT && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
  }

  // Whether something unrelated is on top of the video at (x, y): a pop-up
  // (or another fixed layer) that isn't the video's own. The player's own
  // controls on top of the video don't count.
  function coveredAt(video, x, y) {
    const hit = document.elementsFromPoint(x, y).find((el) => el !== host);
    // From outside a web-component player, the browser reports the player itself: that is the video's own.
    if (!hit || hit === video || within(hit, video)) return false;
    for (let el = hit; el && el !== document.documentElement; el = el.parentElement) {
      if (getComputedStyle(el).position === "fixed") return !within(el, video);
    }
    return false;
  }

  // Covered where the bar goes (top right) and in the middle, like a page's
  // preview video behind a pop-up showing the chosen one.
  function covered(video) {
    const r = video.getBoundingClientRect();
    const top = Math.max(r.top, 0);
    return (
      coveredAt(video, Math.min(r.right, innerWidth) - 30, top + 20) &&
      coveredAt(video, (Math.max(r.left, 0) + Math.min(r.right, innerWidth)) / 2, (top + Math.min(r.bottom, innerHeight)) / 2)
    );
  }

  // Where the mouse is; the bar goes on the video under it, like IDM.
  let pointer = null;

  // The video the bar goes on: the one under the mouse; else the one it was
  // on; else one that's playing, then the largest. Videos behind someone
  // else's pop-up are passed over, unless every video seems covered.
  function pick() {
    const videos = videosInPage().filter(usable);
    if (videos.length === 0) return null;
    if (pointer) {
      const [x, y] = pointer;
      const under = videos.find((v) => {
        const r = v.getBoundingClientRect();
        return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom && !coveredAt(v, x, y);
      });
      if (under) return under;
    }
    if (current && videos.includes(current) && !covered(current)) return current;
    videos.sort((a, b) => (b.paused ? 0 : 1) - (a.paused ? 0 : 1) || area(b) - area(a));
    return videos.find((v) => !covered(v)) || videos[0];
  }

  function area(video) {
    const r = video.getBoundingClientRect();
    return r.width * r.height;
  }

  // Pages that scale or shift themselves (zoom, transform, filter on the
  // page) move the bar too, so its screen position is measured and corrected.
  function moveTo(x, y) {
    host.style.left = "0px";
    host.style.top = "0px";
    const origin = host.getBoundingClientRect();
    host.style.left = "100px";
    host.style.top = "100px";
    const moved = host.getBoundingClientRect();
    const scaleX = (moved.left - origin.left) / 100 || 1;
    const scaleY = (moved.top - origin.top) / 100 || 1;
    host.style.left = `${(x - origin.left) / scaleX}px`;
    host.style.top = `${(y - origin.top) / scaleY}px`;
    return moved.width;
  }

  function place() {
    if (count === 0) return hide();
    const video = pick();
    if (!video) return hide();
    if (!host) build();
    current = video;
    // In fullscreen only the fullscreen element is shown, so the bar moves into it.
    const container = document.fullscreenElement && document.fullscreenElement.tagName !== "VIDEO" ? document.fullscreenElement : document.documentElement;
    if (host.parentNode !== container) container.appendChild(host);
    host.style.display = "block";
    const r = video.getBoundingClientRect();
    const width = moveTo(0, 0) || 200;
    moveTo(Math.max(r.right - width - 10, r.left + 10), Math.max(r.top, 0) + 10);
  }

  // "28 min 47 sec", like IDM.
  function length(seconds) {
    if (!seconds) return null;
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h) return `${h} hr ${m} min`;
    return m ? `${m} min ${s} sec` : `${s} sec`;
  }

  // "The Tick (2016-2019) - 1Shows, TS file, 28 min 47 sec"
  function describe(video, title) {
    const parts = [title || hostOf(video.url)];
    if (video.kind === "stream") {
      parts.push(`${video.format || "Video"} ${video.format ? "file" : "stream"}`, video.quality, length(video.duration));
    } else {
      parts.push(`${video.ext.toUpperCase()} file`, video.size ? `${(video.size / 1048576).toFixed(1)} MB` : null);
    }
    return parts.filter(Boolean).join(", ");
  }

  function hostOf(url) {
    try {
      return new URL(url).hostname;
    } catch {
      return "video";
    }
  }

  function flash(ok, text) {
    bar.classList.add(ok ? "ok" : "bad");
    label.textContent = text || (ok ? "Sent to BDownloader" : "BDownloader isn't running");
    setTimeout(() => {
      bar.classList.remove("ok", "bad");
      label.textContent = "Download this video";
      place();
    }, 2500);
  }

  // Sends the videos to the app, one after another.
  async function send(videos) {
    menu.style.display = "none";
    let ok = true;
    for (const video of videos) {
      ok = Boolean(await ask({ type: "download-from-page", video }).catch(() => false)) && ok;
    }
    flash(ok, ok && videos.length > 1 ? `Sent ${videos.length} videos to BDownloader` : null);
  }

  function menuItem(text, onClick, className) {
    const item = document.createElement("button");
    item.textContent = text;
    if (className) item.className = className;
    item.addEventListener("click", onClick);
    return item;
  }

  async function onDownloadClick() {
    if (menu.style.display === "block") {
      menu.style.display = "none";
      return;
    }
    if (!connected()) return flash(false, "Reload this page");
    const answer = (await ask({ type: "videos-here" }).catch(() => null)) || {};
    const title = answer.title || "";
    // A YouTube video is offered by the player's own frame, which knows its formats.
    let videos = answer.videos || [];
    if (onYoutube && videos.some((v) => v.kind === "youtube")) return youtubeMenu();
    videos = videos.filter((v) => v.kind !== "youtube");
    if (videos.length === 0) return;
    if (videos.length === 1) return send(videos);
    // A video playing a plain file the extension found: that's the one wanted.
    const playing = current && (current.currentSrc || current.src);
    const own = videos.find((v) => v.url === playing);
    if (own) return send([own]);
    menu.replaceChildren(
      menuItem("Download all", () => send(videos), "all"),
      document.createElement("hr"),
      ...videos.map((video, i) => menuItem(`${i + 1}. ${describe(video, title)}`, () => send([video]))),
    );
    menu.style.display = "block";
  }

  // --- YouTube ---------------------------------------------------------------

  const onYoutube = /(^|\.)youtube(-nocookie)?\.com$/.test(location.hostname);

  // YouTube's player streams from a worker the extension can't tie to this
  // tab, so this frame asks whether its player's playback has been picked up.
  if (onYoutube) {
    const check = async () => {
      if (!connected()) return;
      const info = await youtubeInfo();
      if (info && info.cpn) await ask({ type: "youtube-ready", cpn: info.cpn }).catch(() => {});
    };
    setInterval(check, 3000);
    setTimeout(check, 500);
  }

  // The video's title, length and formats, from yt-page.js in this page.
  let nextAsk = 0;
  function youtubeInfo() {
    const id = ++nextAsk;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        window.removeEventListener("message", listen);
        resolve(null);
      }, 1500);
      function listen(event) {
        if (event.source !== window || !event.data || event.data.bdownloader !== "yt-info" || event.data.id !== id) return;
        clearTimeout(timer);
        window.removeEventListener("message", listen);
        resolve(event.data.info);
      }
      window.addEventListener("message", listen);
      window.postMessage({ bdownloader: "yt-ask", id }, "*");
    });
  }

  // "1080p60 MP4, 10 min 35 sec, 254 MB", one line per quality, like IDM.
  async function youtubeMenu() {
    const info = await youtubeInfo();
    if (!info) return flash(false, "Play the video first");
    if (info.live) return flash(false, "Live streams can't be downloaded");
    const choices = (await ask({ type: "youtube-choices", info }).catch(() => [])) || [];
    if (choices.length === 0) return flash(false, "No downloadable format");
    const label = (choice) => {
      const size = [choice.video, choice.audio].reduce((sum, f) => sum + ((f && f.contentLength) || 0), 0);
      const kind = choice.video ? `${choice.video.qualityLabel || choice.height + "p"} MP4` : "Sound only (M4A)";
      return [kind, length(info.lengthSeconds), size ? `${(size / 1048576).toFixed(0)} MB` : null].filter(Boolean).join("  ·  ");
    };
    // The title once on top, then one short line per quality, so long titles
    // don't push what tells the qualities apart out of sight.
    const heading = document.createElement("div");
    heading.className = "heading";
    heading.textContent = info.title;
    menu.replaceChildren(
      heading,
      document.createElement("hr"),
      ...choices.map((choice) =>
        menuItem(label(choice), async () => {
          menu.style.display = "none";
          const ok = await ask({ type: "download-youtube", info, choice }).catch(() => false);
          flash(Boolean(ok), ok ? null : "Play the video for a few seconds, then try again");
        }),
      ),
    );
    menu.style.display = "block";
  }

  chrome.runtime.onMessage.addListener((message, sender, reply) => {
    if (message.type === "videos-count") {
      count = message.count;
      place();
    }
    // The popup downloads a YouTube video from here, where its formats are known.
    if (message.type === "youtube-info" && onYoutube) {
      youtubeInfo().then(reply);
      return true;
    }
    return false;
  });

  ask({ type: "videos-count" })
    .then((n) => {
      count = n || 0;
      place();
    })
    .catch(() => {});

  // Players move and resize; keep the bar on the video.
  for (const event of ["scroll", "resize", "fullscreenchange"]) {
    window.addEventListener(event, () => count && place(), { passive: true, capture: true });
  }
  document.addEventListener("play", () => count && place(), true);
  // The bar follows the mouse from video to video, at most every 100 ms.
  let moveQueued = false;
  document.addEventListener(
    "mousemove",
    (event) => {
      pointer = [event.clientX, event.clientY];
      if (!count || moveQueued || (menu && menu.style.display === "block")) return;
      moveQueued = true;
      setTimeout(() => {
        moveQueued = false;
        place();
      }, 100);
    },
    { passive: true, capture: true },
  );
  const timer = setInterval(() => {
    // A copy left over from before the extension was reloaded steps aside.
    if (!connected()) {
      count = 0;
      hide();
      clearInterval(timer);
      return;
    }
    if (count) place();
  }, 1000);
})();
