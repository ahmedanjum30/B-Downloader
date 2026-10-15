# Privacy policy

This policy covers the BDownloader desktop app and the BDownloader browser extension for Chrome, Edge, Firefox and other browsers.

**Short version:** BDownloader doesn't collect, send or sell any data. Everything stays on your computer.

## What the extension handles

When you start a download in the browser, or choose **Download with BDownloader** from the right-click menu, the extension passes these details to the BDownloader app on the same computer:

- the download link
- the page the download came from (the referrer)
- the file name the browser suggests
- your browser's cookies for that link
- the browser's user agent

The app needs the cookies and referrer to download files from sites where you're signed in, in the same way the browser would.

To show the **Download this video** bar, the extension also notices the videos a page plays: the addresses of video files and streams the page loads, and on YouTube, the video's title, its available formats and the player's streaming request. It keeps these in the browser's memory for that tab only (they're forgotten when you leave the page or close the browser), and passes the one you pick to the app on your computer, the same way as a download.

The extension doesn't read or send the text of the pages you visit, and keeps no history of them.

These details go **only to the BDownloader app on your computer**, through the browser's native messaging channel. The extension doesn't send anything over the internet, and has no server, analytics or tracking.

## What the app stores

The app keeps its download list and settings in `%APPDATA%\BDownloader` on your computer. For each download it stores the link, the file name and folder, and the referrer and user agent, so that it can resume the download later. **Cookies are never written to disk.** They're kept in memory only while the app runs.

The app then connects only to the servers that host the files you download.

## Sharing

BDownloader doesn't share data with anyone. There are no accounts, ads or analytics.

## Contact

Questions about this policy: open an issue at https://github.com/ahmedanjum30/BDownloader/issues.

_Last updated: October 15, 2026_
