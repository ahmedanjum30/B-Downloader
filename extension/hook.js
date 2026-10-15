// Runs inside the page, before its own scripts, and watches the responses
// its video player receives. Players often load their playlist under a
// misleading name and type, so a playlist is recognised by what it starts
// with ("#EXTM3U"). Found playlists are passed to content.js, which tells
// the extension. Nothing is changed or delayed: only a copy is peeked at.
(() => {
  const MARK = "#EXTM3U";
  const MAX_LENGTH = 8 * 1024 * 1024;
  const seen = new Set();

  function report(url) {
    try {
      url = new URL(url, location.href).href;
    } catch {
      return;
    }
    if (seen.has(url)) return;
    seen.add(url);
    // A playlist made inside the page has no address the app could load.
    const kind = /^https?:/i.test(url) ? "playlist" : "inline-playlist";
    window.postMessage({ bdownloader: kind, url }, "*");
  }

  // Chunks of a stream, whatever type the server claims: MPEG-TS packets
  // start with 0x47 every 188 bytes; MP4 fragments with "moof" or "styp".
  function isChunk(bytes) {
    if (bytes.length >= 189 && bytes[0] === 0x47 && bytes[188] === 0x47) return true;
    const box = bytes.length >= 8 ? String.fromCharCode(...bytes.subarray(4, 8)) : "";
    return box === "moof" || box === "styp";
  }

  const chunkHosts = new Set();
  function reportChunk(url) {
    let host;
    try {
      url = new URL(url, location.href);
      host = url.hostname;
    } catch {
      return;
    }
    if (!/^https?:/i.test(url.protocol) || chunkHosts.has(host)) return;
    chunkHosts.add(host);
    window.postMessage({ bdownloader: "chunk", url: url.href }, "*");
  }

  // Video responses: only the first bytes are looked at, to spot chunks.
  function isVideoType(type) {
    return /^\s*video\//i.test(type || "");
  }

  // Video, sound, images and fonts are never playlists.
  function mightBePlaylist(type, length) {
    if (length > MAX_LENGTH) return false;
    return !/^\s*(video|audio|image|font)\//i.test(type || "");
  }

  function startsWithMark(text) {
    return typeof text === "string" && text.replace(/^﻿/, "").trimStart().startsWith(MARK);
  }

  function bytesStartWithMark(bytes) {
    try {
      return startsWithMark(new TextDecoder().decode(bytes.subarray(0, 64)));
    } catch {
      return false;
    }
  }

  const originalFetch = window.fetch;
  if (typeof originalFetch === "function") {
    window.fetch = function (...args) {
      const result = originalFetch.apply(this, args);
      result
        .then((response) => {
          if (!response.body) return;
          const type = response.headers.get("content-type");
          const video = isVideoType(type);
          if (!video && !mightBePlaylist(type, Number(response.headers.get("content-length")))) return;
          // Read only the first bytes of a copy; the page reads the original.
          const reader = response.clone().body.getReader();
          reader
            .read()
            .then(({ value }) => {
              if (!value) return;
              if (video) {
                if (isChunk(value)) reportChunk(response.url);
              } else if (bytesStartWithMark(value)) {
                report(response.url);
              }
            })
            .catch(() => {})
            .finally(() => reader.cancel().catch(() => {}));
        })
        .catch(() => {});
      return result;
    };
  }

  const open = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.addEventListener("load", () => {
      try {
        const type = this.getResponseHeader("content-type");
        const body = this.response;
        if (isVideoType(type) || body instanceof ArrayBuffer) {
          if (body instanceof ArrayBuffer && isChunk(new Uint8Array(body, 0, Math.min(body.byteLength, 200)))) {
            reportChunk(this.responseURL || url);
            return;
          }
          if (isVideoType(type)) return;
        }
        if (!mightBePlaylist(type, 0)) return;
        const found =
          this.responseType === "" || this.responseType === "text"
            ? startsWithMark(this.responseText)
            : body instanceof ArrayBuffer
              ? bytesStartWithMark(new Uint8Array(body))
              : false;
        if (found) report(this.responseURL || url);
      } catch {
        // Some responses can't be read; they aren't playlists we can use.
      }
    });
    return open.call(this, method, url, ...rest);
  };
})();
