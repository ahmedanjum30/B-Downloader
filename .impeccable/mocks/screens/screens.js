// Shared behaviour for the B-Downloader screen replicas.
const SCREENS = [
  ["main.html", "Main"],
  ["queue.html", "Queue"],
  ["torrents.html", "Torrents"],
  ["empty.html", "First run"],
  ["add.html", "Add download"],
  ["add-torrent.html", "Add torrent"],
  ["details.html", "Details"],
  ["details-torrent.html", "Torrent details"],
  ["refresh.html", "Refresh link"],
  ["remove.html", "Remove"],
  ["settings.html", "Settings"],
  ["extension.html", "Browser extension"],
  ["report.html", "Report a problem"],
  ["countdown.html", "When done"],
  ["video-bar.html", "Video bar (extension)"],
];

function storedTheme() {
  try { return localStorage.getItem("bdl-theme"); } catch { return null; }
}
function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  try { localStorage.setItem("bdl-theme", theme); } catch { /* private window */ }
  const b = document.querySelector(".switcher .theme button");
  if (b) b.textContent = theme === "light" ? "Dark theme" : "Light theme";
}
setTheme(new URLSearchParams(location.search).get("theme") || storedTheme() || "dark");

document.addEventListener("DOMContentLoaded", async () => {
  const here = location.pathname.split("/").pop() || "main.html";
  // ?bare: the screen alone, for screenshots.
  if (new URLSearchParams(location.search).has("bare")) document.documentElement.classList.add("bare");
  const nav = document.createElement("nav");
  nav.className = "switcher";
  nav.innerHTML = SCREENS.map(([href, name]) =>
    `<a href="${href}"${href === here ? ' aria-current="page"' : ""}>${name}</a>`).join("") +
    `<span class="theme"><button type="button"></button></span>`;
  document.body.prepend(nav);
  setTheme(document.documentElement.dataset.theme);
  nav.querySelector(".theme button").addEventListener("click", () =>
    setTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light"));

  // Settings tabs.
  document.querySelectorAll("[data-tabs]").forEach((tabs) => {
    tabs.addEventListener("click", (e) => {
      const tab = e.target.closest("button[data-tab]");
      if (!tab) return;
      tabs.querySelectorAll("button[data-tab]").forEach((b) => b.setAttribute("aria-selected", b === tab));
      document.querySelectorAll("[data-panel]").forEach((p) => { p.hidden = p.dataset.panel !== tab.dataset.tab; });
    });
  });
  // Segmented controls.
  document.querySelectorAll(".seg").forEach((seg) => seg.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    seg.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", x === b));
  }));

  // Closing a dialog (X, Cancel, Close) plays the close animation first.
  document.addEventListener("click", (e) => {
    const link = e.target.closest(".dialog a[href='main.html']");
    const scrim = link && link.closest(".scrim");
    if (!scrim || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    scrim.classList.add("closing");
    scrim.addEventListener("animationend", () => { location.href = link.href; }, { once: true });
  });

  // Dialog screens: draw the main window behind the scrim.
  const backdrop = document.querySelector("[data-backdrop]");
  if (backdrop) {
    try {
      const html = await (await fetch("main.html")).text();
      const doc = new DOMParser().parseFromString(html, "text/html");
      const win = doc.querySelector(".window");
      const scrim = backdrop.querySelector(".scrim");
      [...win.children].forEach((child) => backdrop.insertBefore(document.importNode(child, true), scrim));
      backdrop.querySelectorAll(".body, .statusbar, .titlebar").forEach((el) => el.setAttribute("inert", ""));
    } catch { /* the dialog still shows on its own */ }
  }
});
