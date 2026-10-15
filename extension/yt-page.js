// Runs inside YouTube's page. Tells content.js about the video playing:
// its title, length and the formats YouTube offers, which only the page's
// player knows. Answers when asked; changes nothing.
(() => {
  function playerResponse() {
    const player = document.getElementById("movie_player");
    try {
      const response = player && typeof player.getPlayerResponse === "function" ? player.getPlayerResponse() : null;
      if (response && response.streamingData) return response;
    } catch {
      // The player isn't ready yet.
    }
    return window.ytInitialPlayerResponse || null;
  }

  function playbackId() {
    const player = document.getElementById("movie_player");
    try {
      return (player && typeof player.getVideoStats === "function" && player.getVideoStats().cpn) || null;
    } catch {
      return null;
    }
  }

  function summary() {
    const response = playerResponse();
    if (!response || !response.videoDetails || !response.streamingData) return null;
    const details = response.videoDetails;
    if (details.isLive) return { videoId: details.videoId, live: true };
    const formats = (response.streamingData.adaptiveFormats || [])
      .filter((f) => /^(video|audio)\/mp4/.test(f.mimeType || ""))
      .map((f) => ({
        itag: f.itag,
        mimeType: f.mimeType,
        lastModified: String(f.lastModified || ""),
        xtags: f.xtags || null,
        contentLength: f.contentLength ? Number(f.contentLength) : null,
        height: f.height || null,
        fps: f.fps || null,
        qualityLabel: f.qualityLabel || null,
        bitrate: f.bitrate || 0,
        defaultAudio: Boolean(f.audioTrack && f.audioTrack.audioIsDefault),
      }));
    return {
      videoId: details.videoId,
      // This playback's ID, as in the player's streaming requests.
      cpn: playbackId(),
      title: details.title || "",
      lengthSeconds: Number(details.lengthSeconds) || 0,
      formats,
    };
  }

  window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.bdownloader !== "yt-ask") return;
    window.postMessage({ bdownloader: "yt-info", id: event.data.id, info: summary() }, "*");
  });
})();
