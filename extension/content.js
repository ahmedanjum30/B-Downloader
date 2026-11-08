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
    if (!connected()) return Promise.reject(new Error("B-Downloader's extension was reloaded"));
    try {
      return chrome.runtime.sendMessage(message);
    } catch (e) {
      return Promise.reject(e);
    }
  }

  // Magnet links open in BDownloader instead of the browser's "Open this
  // app?" prompt. If BDownloader can't be reached, the browser has the link as usual.
  document.addEventListener(
    "click",
    (event) => {
      if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
      const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
      if (!link || !/^magnet:\?xt=urn:btih:/i.test(link.href) || !connected()) return;
      event.preventDefault();
      const href = link.href;
      ask({ type: "magnet", url: href }).then(
        (taken) => {
          if (!taken) location.href = href;
        },
        () => {
          location.href = href;
        },
      );
    },
    true,
  );

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
  let icon = null;
  let menu = null;

  // Lucide icons (https://lucide.dev, ISC license), as in the app.
  const ICON = {
    download: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>',
    check: '<path d="M20 6 9 17l-5-5"/>',
    alert: '<circle cx="12" cy="12" r="9"/><path d="M12 8v5M12 16h.01"/>',
    chevron: '<path d="m6 9 6 6 6-6"/>',
    close: '<path d="M18 6 6 18M6 6l12 12"/>',
    music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  };
  function svg(name, className = "") {
    return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round">${ICON[name]}</svg>`;
  }

  // The bar sits on videos on any site, so it carries its own colours: always
  // B-Downloader's dark look, readable on any picture.
  function build() {
    host = document.createElement("div");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; top: 0; left: 0; display: none;";
    const root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        :host { --bg: rgba(28, 28, 33, .94); --panel: #1c1c21; --hover: #2c2c34; --line: #34343d; --text: #e6e6ea;
                --muted: #8d8d98; --from: #4a8df6; --to: #a35bf7; --ok: #3fb950; --bad: #f85149; }
        * { box-sizing: border-box; font: 600 11px/14px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; color: var(--text); }
        .bar { display: flex; align-items: center; border-radius: 6px; overflow: hidden; background: var(--bg);
               border: 1px solid rgba(255, 255, 255, .08); box-shadow: 0 6px 20px rgba(0, 0, 0, .35), 0 1px 3px rgba(0, 0, 0, .3); }
        button { all: unset; box-sizing: border-box; cursor: pointer; display: flex; align-items: center; }
        button:focus-visible { outline: 2px solid var(--from); outline-offset: -2px; }
        .main { gap: 5px; height: 18px; padding: 0 8px; color: #fff; background: linear-gradient(90deg, var(--from), var(--to)); }
        .main:hover { filter: brightness(1.08); }
        .main * { color: #fff; }
        /* Segoe UI's lowercase sits low in its line: centred on the capitals,
           the label reads below the middle, so it's lifted a pixel. */
        .label { position: relative; top: -1px; }
        .main.ok { background: var(--ok); }
        .main.bad { background: var(--bad); }
        .close { width: 22px; height: 18px; justify-content: center; color: var(--muted); }
        .close:hover { background: var(--hover); }
        .close svg { width: 11px; height: 11px; }
        svg { width: 12px; height: 12px; flex: none; }
        .chevron { width: 11px; height: 11px; opacity: .85; }
        .menu { position: absolute; right: 0; top: 100%; margin-top: 6px; width: 320px; max-width: 90vw; border-radius: 12px;
                background: var(--panel); border: 1px solid var(--line); overflow: hidden; display: none;
                box-shadow: 0 16px 40px rgba(0, 0, 0, .5), 0 2px 6px rgba(0, 0, 0, .3);
                animation: open 160ms cubic-bezier(.16, 1, .3, 1) both; transform-origin: top right; }
        @keyframes open { from { opacity: 0; transform: translateY(-4px) scale(.98); } }
        @media (prefers-reduced-motion: reduce) { .menu { animation: none; } }
        .head { padding: 12px 14px 10px; border-bottom: 1px solid var(--line); }
        .title { font-size: 13px; line-height: 18px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
        .meta { margin-top: 2px; color: var(--muted); font-weight: 400; font-size: 12px; line-height: 16px; font-variant-numeric: tabular-nums; }
        .list { padding: 4px; display: flex; flex-direction: column; max-height: 60vh; overflow-y: auto; }
        .row { display: grid; grid-template-columns: 52px 1fr auto; gap: 8px; height: 28px; padding: 0 10px; border-radius: 8px; font-size: 12.5px; line-height: 16px; }
        .row:hover { background: var(--hover); }
        .row:hover .size { color: var(--text); }
        .row.plain { display: block; height: auto; padding: 6px 10px; font-weight: 400; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
        .row.all { font-weight: 600; }
        .q { font-size: 12.5px; font-variant-numeric: tabular-nums; display: flex; align-items: center; }
        .q svg { width: 14px; height: 14px; color: var(--muted); }
        .fmt { display: flex; align-items: center; gap: 6px; color: var(--muted); font-weight: 400; font-size: 12.5px; }
        .best { font-size: 10.5px; line-height: 16px; padding: 0 6px; border-radius: 999px; color: #fff;
                background: linear-gradient(90deg, var(--from), var(--to)); }
        .size { color: var(--muted); font-weight: 400; font-size: 12.5px; font-variant-numeric: tabular-nums; text-align: right; }
        .sep { height: 1px; background: var(--line); margin: 4px 6px; }
      </style>
      <div class="bar">
        <button class="main" title="Download with B-Downloader" aria-haspopup="menu">
          <span class="icon">${svg("download")}</span><span class="label">Download this video</span>
        </button>
        <button class="close" title="Hide on this video" aria-label="Hide on this video">${svg("close")}</button>
      </div>
      <div class="menu" role="menu"></div>`;
    bar = root.querySelector(".main");
    label = root.querySelector(".main .label");
    icon = root.querySelector(".main .icon");
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
    icon.innerHTML = svg(ok ? "check" : "alert");
    label.textContent = text || (ok ? "Sent to B-Downloader" : "B-Downloader isn't running");
    setTimeout(() => {
      bar.classList.remove("ok", "bad");
      icon.innerHTML = svg("download");
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
    flash(ok, ok && videos.length > 1 ? `Sent ${videos.length} videos to B-Downloader` : null);
  }

  function menuItem(text, onClick, className) {
    const item = document.createElement("button");
    item.textContent = text;
    item.className = `row plain ${className || ""}`;
    item.setAttribute("role", "menuitem");
    item.addEventListener("click", onClick);
    return item;
  }

  function separator() {
    const line = document.createElement("div");
    line.className = "sep";
    line.setAttribute("role", "separator");
    return line;
  }

  // The menu: a heading (the title, and a line under it), then the choices.
  function showMenu(title, meta, items) {
    const list = document.createElement("div");
    list.className = "list";
    list.replaceChildren(...items);
    const parts = [list];
    if (title) {
      const head = document.createElement("div");
      head.className = "head";
      const t = document.createElement("div");
      t.className = "title";
      t.textContent = title;
      head.append(t);
      if (meta) {
        const m = document.createElement("div");
        m.className = "meta";
        m.textContent = meta;
        head.append(m);
      }
      parts.unshift(head);
    }
    menu.replaceChildren(...parts);
    menu.style.display = "block";
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
    showMenu(title, `${videos.length} videos on this page`, [
      menuItem("Download all", () => send(videos), "all"),
      separator(),
      ...videos.map((video, i) => menuItem(`${i + 1}. ${describe(video, title)}`, () => send([video]))),
    ]);
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
    // The title and length once on top, then one row per quality: what
    // tells them apart (quality and size) lines up in columns.
    const row = (choice, best) => {
      const size = [choice.video, choice.audio].reduce((sum, f) => sum + ((f && f.contentLength) || 0), 0);
      const item = document.createElement("button");
      item.className = "row";
      item.setAttribute("role", "menuitem");
      const q = document.createElement("span");
      q.className = "q";
      const fmt = document.createElement("span");
      fmt.className = "fmt";
      if (choice.video) {
        q.textContent = choice.video.qualityLabel || `${choice.height}p`;
        fmt.textContent = "MP4";
        if (best) fmt.insertAdjacentHTML("beforeend", '<span class="best">Best</span>');
      } else {
        q.innerHTML = svg("music");
        fmt.textContent = "Sound only · M4A";
      }
      const sz = document.createElement("span");
      sz.className = "size";
      sz.textContent = size ? `${(size / 1048576).toFixed(0)} MB` : "";
      item.append(q, fmt, sz);
      item.addEventListener("click", async () => {
        menu.style.display = "none";
        const ok = await ask({ type: "download-youtube", info, choice }).catch(() => false);
        flash(Boolean(ok), ok ? null : "Play the video for a few seconds, then try again");
      });
      return item;
    };
    const videos = choices.filter((c) => c.video);
    const sound = choices.filter((c) => !c.video);
    showMenu(info.title, [length(info.lengthSeconds), "Goes to B-Downloader"].filter(Boolean).join(" · "), [
      ...videos.map((choice, i) => row(choice, i === 0)),
      ...(videos.length && sound.length ? [separator()] : []),
      ...sound.map((choice) => row(choice, false)),
    ]);
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
