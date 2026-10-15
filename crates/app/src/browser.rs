//! Browser integration: registers the bridge program (`bdl-host.exe`) with
//! Chromium-based browsers and Firefox, and finds the extension folders.

use std::io;
use std::path::PathBuf;

/// The name the extension uses to reach the bridge.
pub const HOST_NAME: &str = "com.bdownloader.host";

/// The Firefox add-on's ID (set in `scripts/firefox-extension.ps1`).
pub const FIREFOX_ADDON_ID: &str = "bdownloader@ahmedanjum30";

/// The browsers the Browser extension dialog has steps for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Edge,
    Brave,
    Opera,
    Vivaldi,
    Firefox,
}

impl Browser {
    pub const ALL: [Browser; 6] = [
        Browser::Chrome,
        Browser::Edge,
        Browser::Brave,
        Browser::Opera,
        Browser::Vivaldi,
        Browser::Firefox,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Edge => "Edge",
            Browser::Brave => "Brave",
            Browser::Opera => "Opera",
            Browser::Vivaldi => "Vivaldi",
            Browser::Firefox => "Firefox",
        }
    }

    /// The page to add the extension on. Browsers open these only when typed
    /// or pasted: they refuse them from other programs.
    pub fn extensions_page(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome://extensions",
            Browser::Edge => "edge://extensions",
            Browser::Brave => "brave://extensions",
            Browser::Opera => "opera://extensions",
            Browser::Vivaldi => "vivaldi://extensions",
            Browser::Firefox => "about:debugging#/runtime/this-firefox",
        }
    }

    /// Where the Developer mode switch is on that page (Chromium browsers).
    pub fn developer_mode(self) -> &'static str {
        match self {
            Browser::Edge => "Turn on Developer mode (in the panel on the left).",
            _ => "Turn on Developer mode (top right).",
        }
    }

    /// The extension's page on the store this browser installs from, once
    /// it's published there. Brave, Opera and Vivaldi use the Chrome Web Store.
    pub fn store_page(self) -> Option<String> {
        match self {
            Browser::Firefox => None,
            Browser::Edge => (!EDGE_STORE_ID.is_empty()).then(|| format!("https://microsoftedge.microsoft.com/addons/detail/{EDGE_STORE_ID}")),
            _ => (!CHROME_STORE_ID.is_empty()).then(|| format!("https://chromewebstore.google.com/detail/{CHROME_STORE_ID}")),
        }
    }

    /// The browser's program, as Windows' "App Paths" knows it.
    fn exe(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome.exe",
            Browser::Edge => "msedge.exe",
            Browser::Brave => "brave.exe",
            Browser::Opera => "opera.exe",
            Browser::Vivaldi => "vivaldi.exe",
            Browser::Firefox => "firefox.exe",
        }
    }

    /// Whether the app can add the extension with one click: Firefox's
    /// signed add-on, or a store page to open.
    pub fn one_click(self) -> bool {
        match self {
            Browser::Firefox => firefox_addon().is_some(),
            _ => self.store_page().is_some(),
        }
    }
}

/// The extension's IDs on the Chrome Web Store and Edge Add-ons, assigned on
/// the first upload; empty until then. Also set in `installer/bdownloader.iss`,
/// which has the browsers offer the extension at their next start.
pub const CHROME_STORE_ID: &str = "";
pub const EDGE_STORE_ID: &str = "kfbjnlpmchkeaphhdeidpdpcomoohdfa";

/// The extension IDs the bridge accepts; it refuses every other extension.
pub const EXTENSION_IDS: &[&str] = &[
    // Loaded unpacked from the `extension` folder (fixed by the `key` in its manifest).
    "glehdmgljkbkfnbigknlpjjmfegaagfi",
    CHROME_STORE_ID,
    EDGE_STORE_ID,
];

/// Where each browser looks for native messaging hosts, under `HKEY_CURRENT_USER`,
/// with the key that shows the browser has been used on this PC.
const BROWSERS: &[(&str, &str)] = &[
    (r"Software\Google\Chrome", r"Software\Google\Chrome\NativeMessagingHosts"),
    (r"Software\Microsoft\Edge", r"Software\Microsoft\Edge\NativeMessagingHosts"),
    (
        r"Software\BraveSoftware\Brave-Browser",
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
    ),
    (r"Software\Chromium", r"Software\Chromium\NativeMessagingHosts"),
    // Opera and Vivaldi are said to read Chrome's location; they're
    // registered under their own and Chromium's too, which costs nothing,
    // so they work whichever they read and also without Chrome installed.
    (r"Software\Opera Software", r"Software\Google\Chrome\NativeMessagingHosts"),
    (r"Software\Opera Software", r"Software\Opera Software\NativeMessagingHosts"),
    (r"Software\Vivaldi", r"Software\Google\Chrome\NativeMessagingHosts"),
    (r"Software\Vivaldi", r"Software\Chromium\NativeMessagingHosts"),
    (r"Software\Vivaldi", r"Software\Vivaldi\NativeMessagingHosts"),
];

fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_owned())
}

/// The extension folder: next to the app when installed, or in the source
/// tree for development builds.
pub fn extension_dir() -> Option<PathBuf> {
    let installed = exe_dir()?.join("extension");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension");
    [installed, source]
        .into_iter()
        .find(|dir| dir.join("manifest.json").exists())
        .and_then(|dir| dir.canonicalize().ok())
        .map(strip_verbatim)
}

/// The Firefox add-on, signed by Mozilla so Firefox keeps it: next to the
/// app when installed, or where `scripts/build-installer.ps1` puts it for
/// development builds.
pub fn firefox_addon() -> Option<PathBuf> {
    let installed = exe_dir()?.join("bdownloader-firefox.xpi");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/bdownloader-firefox.xpi");
    [installed, source]
        .into_iter()
        .find(|file| file.exists())
        .and_then(|file| file.canonicalize().ok())
        .map(strip_verbatim)
}

/// Opens the extension in the browser, which then asks whether to add it:
/// Firefox gets the signed add-on, the others its store page.
pub fn add_extension(browser: Browser) -> io::Result<()> {
    let target = match browser {
        Browser::Firefox => firefox_addon()
            .ok_or_else(|| io::Error::other("the Firefox add-on is missing"))?
            .into_os_string(),
        _ => browser.store_page().ok_or_else(|| io::Error::other("not on the store yet"))?.into(),
    };
    match browser_exe(browser) {
        Some(exe) => std::process::Command::new(exe).arg(target).spawn().map(drop),
        // A store page opens in the default browser too; the add-on doesn't.
        None if browser != Browser::Firefox => std::process::Command::new("explorer").arg(target).spawn().map(drop),
        None => Err(io::Error::other(format!("{} isn't installed", browser.name()))),
    }
}

/// Where the browser is installed. Browser installers record it, for the
/// whole PC or one user.
#[cfg(windows)]
fn browser_exe(browser: Browser) -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .filter_map(|root| {
            RegKey::predef(root)
                .open_subkey(format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}", browser.exe()))
                .ok()?
                .get_value::<String, _>("")
                .ok()
        })
        .map(|path| PathBuf::from(path.trim_matches('"')))
        .find(|path| path.exists())
}

#[cfg(not(windows))]
fn browser_exe(_browser: Browser) -> Option<PathBuf> {
    None
}

/// Where Firefox looks for native messaging hosts, under `HKEY_CURRENT_USER`.
const FIREFOX_HOSTS: &str = r"Software\Mozilla\NativeMessagingHosts";

/// `canonicalize` returns `\\?\C:\...`, which Explorer and Chrome don't accept.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(plain) => PathBuf::from(plain),
        None => path,
    }
}

/// Writes the host manifest and points every installed Chromium browser at
/// it. Runs at every start, so the registration follows the app if it moves.
#[cfg(windows)]
pub fn register() -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let host = exe_dir().ok_or_else(|| io::Error::other("no exe folder"))?.join("bdl-host.exe");
    if !host.exists() {
        return Ok(());
    }
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "BDownloader",
        "path": host,
        "type": "stdio",
        "allowed_origins": EXTENSION_IDS.iter().filter(|id| !id.is_empty()).map(|id| format!("chrome-extension://{id}/")).collect::<Vec<_>>(),
    });
    let dir = crate::store::data_dir();
    std::fs::create_dir_all(&dir)?;
    let manifest_path = dir.join(format!("{HOST_NAME}.json"));
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    let user = RegKey::predef(HKEY_CURRENT_USER);
    for &(browser, hosts) in BROWSERS {
        if user.open_subkey(browser).is_ok() {
            let (key, _) = user.create_subkey(format!(r"{hosts}\{HOST_NAME}"))?;
            key.set_value("", &manifest_path.as_os_str())?;
        }
    }

    // Firefox names the allowed add-on by its ID instead of an origin, so it
    // gets its own copy. Always registered: Firefox may be installed later.
    let firefox = serde_json::json!({
        "name": HOST_NAME,
        "description": "BDownloader",
        "path": host,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_ADDON_ID],
    });
    let firefox_path = dir.join(format!("{HOST_NAME}.firefox.json"));
    std::fs::write(&firefox_path, serde_json::to_vec_pretty(&firefox)?)?;
    let (key, _) = user.create_subkey(format!(r"{FIREFOX_HOSTS}\{HOST_NAME}"))?;
    key.set_value("", &firefox_path.as_os_str())?;
    Ok(())
}

/// Removes what [`register`] added. Run by the uninstaller.
#[cfg(windows)]
pub fn unregister() -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let user = RegKey::predef(HKEY_CURRENT_USER);
    for hosts in BROWSERS.iter().map(|&(_, hosts)| hosts).chain([FIREFOX_HOSTS]) {
        if let Ok(key) = user.open_subkey_with_flags(hosts, winreg::enums::KEY_ALL_ACCESS) {
            let _ = key.delete_subkey_all(HOST_NAME);
        }
    }
    for file in [format!("{HOST_NAME}.json"), format!("{HOST_NAME}.firefox.json")] {
        match std::fs::remove_file(crate::store::data_dir().join(file)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register() -> io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn unregister() -> io::Result<()> {
    Ok(())
}
