# B-Downloader in the Microsoft Store: unvirtualized resources

This page answers the Microsoft Store certification questions about the
`unvirtualizedResources` restricted capability of the B-Downloader MSIX package
(identity `AhmadAnjum.B-Downloader`). Everything here can be checked against the
source code in this repository; file and line references are given.

## Why the capability is needed

B-Downloader's browser extension (published on Microsoft Edge Add-ons and
Firefox Add-ons) hands downloads to the app through **browser native
messaging**. Browsers run outside the app's package, so they can only find the
app's native messaging host through real `HKEY_CURRENT_USER` registry keys and a
manifest file on disk. That is the only use of the capability.

## 1. Product IDs: 9NDM855XC7FC and XP89FXJJF4J45W

- **XP89FXJJF4J45W** ("BDownloader") is the **older listing of the unpackaged
  Win32 installer** (an EXE installer, live since October 2026). It can no
  longer be updated because it requires a code-signed installer (policy 10.2.9).
- **9NDM855XC7FC** ("B-Downloader") is the **MSIX package listing that replaces
  it**. It is the one under review.

The links to XP89FXJJF4J45W have been removed from the website and this
repository, which now say a Microsoft Store version is coming soon. Once
9NDM855XC7FC is approved, they will link to it, and the XP89FXJJF4J45W listing
will be made unavailable.

## 2. The seven registry keys

Each is a single key under `HKEY_CURRENT_USER`, one per supported browser:

1. Google Chrome: `HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\com.bdownloader.host`
2. Microsoft Edge: `HKEY_CURRENT_USER\Software\Microsoft\Edge\NativeMessagingHosts\com.bdownloader.host`
3. Brave: `HKEY_CURRENT_USER\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.bdownloader.host`
4. Chromium: `HKEY_CURRENT_USER\Software\Chromium\NativeMessagingHosts\com.bdownloader.host`
5. Opera: `HKEY_CURRENT_USER\Software\Opera Software\NativeMessagingHosts\com.bdownloader.host`
6. Vivaldi: `HKEY_CURRENT_USER\Software\Vivaldi\NativeMessagingHosts\com.bdownloader.host`
7. Firefox: `HKEY_CURRENT_USER\Software\Mozilla\NativeMessagingHosts\com.bdownloader.host`

A Chromium-based browser's key is written only if that browser is installed
(its `HKEY_CURRENT_USER\Software\<browser>` key exists). The Firefox key is
always written. Source: `BROWSERS` and `FIREFOX_HOSTS` and `register()` in
[`crates/app/src/browser.rs`](../crates/app/src/browser.rs).

## 3. The two files

Both in `%APPDATA%\BDownloader\NativeMessaging` (for example
`C:\Users\<name>\AppData\Roaming\BDownloader\NativeMessaging`):

- `%APPDATA%\BDownloader\NativeMessaging\com.bdownloader.host.json`: the
  native messaging manifest for Chromium-based browsers (keys 1 to 6)
- `%APPDATA%\BDownloader\NativeMessaging\com.bdownloader.host.firefox.json`:
  the native messaging manifest for Firefox (key 7)

No other file is written to that folder.

## 4. What each key points to

Each key holds only its default value, which is the full path of one of the
two files above:

- keys 1 to 6 (Chrome, Edge, Brave, Chromium, Opera, Vivaldi):
  `%APPDATA%\BDownloader\NativeMessaging\com.bdownloader.host.json`
- key 7 (Firefox):
  `%APPDATA%\BDownloader\NativeMessaging\com.bdownloader.host.firefox.json`

No other values are written.

## 5. What the manifests allow and launch

- **Launch:** both manifests' `"path"` is the package's own execution alias,
  `%LOCALAPPDATA%\Microsoft\WindowsApps\bdl-host.exe` (declared as
  `windows.appExecutionAlias` for `bdl-host.exe` in the package manifest). They
  launch nothing else.
- **Allowed extensions, Chromium manifest (`allowed_origins`):**
  - `chrome-extension://kfbjnlpmchkeaphhdeidpdpcomoohdfa/`: the published
    B-Downloader extension on Microsoft Edge Add-ons
  - `chrome-extension://glehdmgljkbkfnbigknlpjjmfegaagfi/`: the same
    official extension loaded from the folder the app ships, for Chrome,
    Brave, Opera, Vivaldi and Chromium (its ID is fixed by the `key` in the
    extension's manifest)
- **Allowed extensions, Firefox manifest (`allowed_extensions`):**
  `bdownloader@ahmedanjum30`, the published B-Downloader add-on on Firefox
  Add-ons

No other extension IDs are allowed. Source: `EXTENSION_IDS`,
`FIREFOX_ADDON_ID` and `register()` in
[`crates/app/src/browser.rs`](../crates/app/src/browser.rs).

## 6. Nothing else is modified

The Store version writes **no** `HKEY_LOCAL_MACHINE` keys, browser policies,
other extensions' registrations, system folders, or other applications' data.
The only unvirtualized writes are the seven keys and two files above.

All other data (settings, download list, resume data, logs) is written to the
package's own virtualized storage and removed with it. The app's one other
registry write (a notification display name under
`HKEY_CURRENT_USER\Software\Classes\AppUserModelId`) is skipped in the Store
version (`crates/app/src/notify.rs`). The only `HKEY_LOCAL_MACHINE` access is a
read of the Windows version for bug reports (`crates/app/src/app/report.rs`).

## 7. The package's exclusion declarations

From [`installer/msix/AppxManifest.xml`](../installer/msix/AppxManifest.xml):

```xml
<virtualization:FileSystemWriteVirtualization>
  <virtualization:ExcludedDirectories>
    <virtualization:ExcludedDirectory>$(KnownFolder:RoamingAppData)\BDownloader\NativeMessaging</virtualization:ExcludedDirectory>
  </virtualization:ExcludedDirectories>
</virtualization:FileSystemWriteVirtualization>
<virtualization:RegistryWriteVirtualization>
  <virtualization:ExcludedKeys>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Google\Chrome\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Microsoft\Edge\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Chromium\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Opera Software\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Vivaldi\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
    <virtualization:ExcludedKey>HKEY_CURRENT_USER\Software\Mozilla\NativeMessagingHosts\com.bdownloader.host</virtualization:ExcludedKey>
  </virtualization:ExcludedKeys>
</virtualization:RegistryWriteVirtualization>
```

## 8. Windows 10

There is no Windows 10 fallback: the package declares
`<TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" ... />`,
so it installs only on Windows 11, where the fine-grained exclusions above are
supported. It never asks for unvirtualized access to anything broader.
Windows 10 users use the separate installer from the website, which is not
distributed through the Store.

## 9. Uninstalling

Uninstall instructions, listing every key and file that remains, with a
PowerShell snippet that removes them:
<https://github.com/ahmedanjum30/B-Downloader#uninstalling>

The seven keys and two files listed above are the complete set of leftovers:
excluded locations persist after an MSIX package is uninstalled, as Windows
documents for flexible virtualization. They are inert without the app: the
manifests point to the package's execution alias, which is removed with the
package, so browsers can start nothing and the extension reports that
B-Downloader is not installed.
