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
            Browser::Firefox => Some(FIREFOX_STORE_PAGE.to_owned()),
            Browser::Edge => (!EDGE_STORE_ID.is_empty()).then(|| format!("https://microsoftedge.microsoft.com/addons/detail/{EDGE_STORE_ID}")),
            _ => (!CHROME_STORE_ID.is_empty()).then(|| format!("https://chromewebstore.google.com/detail/{CHROME_STORE_ID}")),
        }
    }

    /// The browser's program, as Windows' "App Paths" knows it.
    #[cfg(windows)]
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

    /// Whether the app can add the extension with one click: the browser's
    /// store has it.
    pub fn one_click(self) -> bool {
        self.store_page().is_some()
    }
}

/// The add-on's page on Firefox Add-ons (addons.mozilla.org).
pub const FIREFOX_STORE_PAGE: &str = "https://addons.mozilla.org/firefox/addon/bdownloader/";

/// The extension's IDs on the Chrome Web Store and Edge Add-ons, assigned on
/// the first upload; empty until then. Also set in `installer/bdownloader.toml`,
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
#[cfg(windows)]
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
/// tree for development builds. On macOS, a copy of the one in the app's
/// bundle: a browser's folder picker can't look inside an app.
pub fn extension_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        static COPY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        COPY.get_or_init(|| {
            let copy = copy_extension(&shipped_extension_dir()?);
            if let Err(e) = &copy {
                log::warn!("couldn't copy the browser extension out of the app: {e}");
            }
            copy.ok()
        })
        .clone()
    }
    #[cfg(not(target_os = "macos"))]
    shipped_extension_dir()
}

/// Copies the extension to B-Downloader's own folder (Application Support),
/// fresh at every start, so the browser loads this version's. It stays put
/// when Homebrew replaces the app.
#[cfg(target_os = "macos")]
fn copy_extension(from: &std::path::Path) -> io::Result<PathBuf> {
    fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> io::Result<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
    let to = crate::store::data_dir().join("extension");
    // Files a newer version dropped shouldn't linger.
    match std::fs::remove_dir_all(&to) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    copy_dir(from, &to)?;
    Ok(to)
}

/// The extension folder as shipped: next to BDownloader.exe, in the app
/// bundle's Resources on macOS, or in the source tree.
fn shipped_extension_dir() -> Option<PathBuf> {
    let installed = exe_dir()?.join("extension");
    let bundled = exe_dir()?.join("../Resources/extension");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension");
    [installed, bundled, source]
        .into_iter()
        .find(|dir| dir.join("manifest.json").exists())
        .and_then(|dir| dir.canonicalize().ok())
        .map(strip_verbatim)
}

/// Opens the extension's store page in the browser, where one click adds it.
#[cfg(target_os = "macos")]
pub fn add_extension(browser: Browser) -> io::Result<()> {
    let page = browser.store_page().ok_or_else(|| io::Error::other("not on the store yet"))?;
    let app = match browser {
        Browser::Chrome => "Google Chrome",
        Browser::Edge => "Microsoft Edge",
        Browser::Brave => "Brave Browser",
        Browser::Opera => "Opera",
        Browser::Vivaldi => "Vivaldi",
        Browser::Firefox => "Firefox",
    };
    let opened = std::process::Command::new("open").args(["-a", app, &page]).status()?;
    if opened.success() {
        return Ok(());
    }
    // Not installed: the default browser opens it instead.
    std::process::Command::new("open").arg(page).spawn().map(drop)
}

/// Opens the extension's store page in the browser, where one click adds it.
#[cfg(not(target_os = "macos"))]
pub fn add_extension(browser: Browser) -> io::Result<()> {
    let page = browser.store_page().ok_or_else(|| io::Error::other("not on the store yet"))?;
    match browser_exe(browser) {
        Some(exe) => std::process::Command::new(exe).arg(page).spawn().map(drop),
        // Not found: the default browser opens it instead.
        None => std::process::Command::new("explorer").arg(page).spawn().map(drop),
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

#[cfg(not(any(windows, target_os = "macos")))]
fn browser_exe(_browser: Browser) -> Option<PathBuf> {
    None
}

/// Where Firefox looks for native messaging hosts, under `HKEY_CURRENT_USER`.
#[cfg(windows)]
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

    let host = if crate::update::is_packaged() {
        // The Store version's files sit in a protected folder that changes
        // with every update; browsers start the bridge through its app alias,
        // which stays put. (An alias is a special link, so it isn't checked
        // with `exists`.)
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or_else(|| io::Error::other("no LOCALAPPDATA"))?)
            .join(r"Microsoft\WindowsApps\bdl-host.exe")
    } else {
        let host = exe_dir().ok_or_else(|| io::Error::other("no exe folder"))?.join("bdl-host.exe");
        if !host.exists() {
            return Ok(());
        }
        host
    };
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

/// Where each browser looks for native messaging hosts on macOS, under
/// `~/Library/Application Support`. A browser's folder is there once it has
/// been used; Firefox's `Mozilla` folder may be created by us.
#[cfg(target_os = "macos")]
const MAC_BROWSERS: &[&str] = &[
    "Google/Chrome",
    "Microsoft Edge",
    "BraveSoftware/Brave-Browser",
    "Chromium",
    "Vivaldi",
    "com.operasoftware.Opera",
];

#[cfg(target_os = "macos")]
fn application_support() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("no HOME"))?;
    Ok(PathBuf::from(home).join("Library/Application Support"))
}

/// Writes the host manifest into every used Chromium browser's folder, and
/// Firefox's. Runs at every start, so the registration follows the app if it
/// moves (dragged to another folder, or upgraded by Homebrew).
#[cfg(target_os = "macos")]
pub fn register() -> io::Result<()> {
    let host = exe_dir().ok_or_else(|| io::Error::other("no exe folder"))?.join("bdl-host");
    if !host.exists() {
        return Ok(());
    }
    let chromium = serde_json::to_vec_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "BDownloader",
        "path": host,
        "type": "stdio",
        "allowed_origins": EXTENSION_IDS.iter().filter(|id| !id.is_empty()).map(|id| format!("chrome-extension://{id}/")).collect::<Vec<_>>(),
    }))?;
    let firefox = serde_json::to_vec_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "BDownloader",
        "path": host,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_ADDON_ID],
    }))?;
    let support = application_support()?;
    for browser in MAC_BROWSERS {
        let dir = support.join(browser);
        if dir.is_dir() {
            let hosts = dir.join("NativeMessagingHosts");
            std::fs::create_dir_all(&hosts)?;
            std::fs::write(hosts.join(format!("{HOST_NAME}.json")), &chromium)?;
        }
    }
    // Always registered: Firefox may be installed later.
    let hosts = support.join("Mozilla/NativeMessagingHosts");
    std::fs::create_dir_all(&hosts)?;
    std::fs::write(hosts.join(format!("{HOST_NAME}.json")), &firefox)
}

/// Removes what [`register`] added.
#[cfg(target_os = "macos")]
pub fn unregister() -> io::Result<()> {
    let support = application_support()?;
    for browser in MAC_BROWSERS.iter().copied().chain(["Mozilla"]) {
        match std::fs::remove_file(support.join(browser).join("NativeMessagingHosts").join(format!("{HOST_NAME}.json"))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn register() -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn unregister() -> io::Result<()> {
    Ok(())
}
