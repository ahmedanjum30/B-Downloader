# Store listing: BDownloader extension

Text and answers to paste into the Chrome Web Store and Edge Add-ons dashboards. Upload `dist/bdownloader-extension-<version>.zip` (built by `scripts/package-extension.ps1`).

## Basics

| Field | Value |
|---|---|
| Name | BDownloader |
| Summary (max 132 characters) | Sends your downloads to BDownloader, a fast multi-connection download manager. |
| Category | Chrome: Productivity → Tools · Edge: Productivity |
| Language | English |
| Homepage | https://github.com/ahmedanjum30/BDownloader |
| Support | https://github.com/ahmedanjum30/BDownloader/issues |
| Privacy policy | https://github.com/ahmedanjum30/BDownloader/blob/main/PRIVACY.md |

## Description

```
BDownloader catches the downloads you start in your browser and hands them to the BDownloader app for Windows, which downloads them faster over several connections at once and lets you pause and resume them at any time.

What the extension does:
• Downloads you start in the browser go to BDownloader automatically.
• Right-click any link, image, video or audio and choose "Download with BDownloader".
• Downloads that need you to be signed in still work: the extension passes along the browser's cookies for that link.
• If BDownloader isn't installed or can't be reached, the browser simply downloads the file itself.

Requires the free BDownloader app for Windows: https://github.com/ahmedanjum30/BDownloader

Privacy: the extension sends download details only to the BDownloader app on your own computer. Nothing goes to any server, and there is no tracking or analytics.
```

## Single purpose

```
Hand the downloads the user starts in the browser, or picks from the right-click menu, to the BDownloader download manager app installed on the same computer.
```

## Permission justifications

| Permission | Justification |
|---|---|
| `downloads` | To notice downloads the user starts, pause them while handing them to the BDownloader app, then cancel the browser's copy once the app has taken over, or resume it if the app isn't available. |
| `contextMenus` | Adds the "Download with BDownloader" entry to the right-click menu of links, images, video and audio. |
| `nativeMessaging` | The only way the extension talks to the BDownloader desktop app on the same computer. It sends the download link and details to the app. |
| `cookies` | Reads the browser's cookies for the URL being downloaded, so the app can download files from sites where the user is signed in. Cookies are sent only to the local app and never stored on disk. |
| Host permission `<all_urls>` | Downloads can come from any website, and reading the cookies for a download's URL requires access to that URL's site. The extension doesn't read or change page content. |

**Remote code:** No. All code is in the package.

## Data usage (Chrome "Privacy practices" tab)

Data types the extension handles:
- **Authentication information:** the cookies for the URL being downloaded.
- **Web history:** the download link and the page it came from.

Explain that this data is sent only to the BDownloader app on the user's own computer, through native messaging, and never leaves the device.

Tick all three certifications:
- I do not sell or transfer user data to third parties, outside of the approved use cases.
- I do not use or transfer user data for purposes that are unrelated to my item's single purpose.
- I do not use or transfer user data to determine creditworthiness or for lending purposes.

## Images

All in `store/images/`:

| File | Size | Used for |
|---|---|---|
| `icon-128.png` | 128×128 | Store icon (Chrome takes it from the package too) |
| `logo-300.png` | 300×300 | Edge store logo |
| `screenshot-1.png` … | 1280×800 | Screenshots (Chrome needs at least 1, up to 5) |
| `promo-small-440x280.png` | 440×280 | Small promo tile (required by Chrome) |
| `promo-marquee-1400x560.png` | 1400×560 | Marquee tile (optional, needed to be featured) |

## After the first upload

Each store assigns the extension an ID. Add both IDs to `EXTENSION_IDS` in `crates/app/src/browser.rs` so the app's bridge accepts the published extension, and release a new app version.
