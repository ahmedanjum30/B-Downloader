//! Browser integration: registers the bridge program (`bdl-host.exe`) with
//! Chromium-based browsers, and finds the extension folder.

use std::io;
use std::path::PathBuf;

/// The name the extension uses to reach the bridge.
pub const HOST_NAME: &str = "com.bdownloader.host";

/// The extension IDs the bridge accepts; it refuses every other extension.
/// Each store assigns its own ID on the first upload, so add them here once known.
pub const EXTENSION_IDS: &[&str] = &[
    // Loaded unpacked from the `extension` folder (fixed by the `key` in its manifest).
    "glehdmgljkbkfnbigknlpjjmfegaagfi",
];

/// Where each browser looks for native messaging hosts, under `HKEY_CURRENT_USER`,
/// with the key that shows the browser has been used on this PC.
const BROWSERS: [(&str, &str); 4] = [
    (r"Software\Google\Chrome", r"Software\Google\Chrome\NativeMessagingHosts"),
    (r"Software\Microsoft\Edge", r"Software\Microsoft\Edge\NativeMessagingHosts"),
    (
        r"Software\BraveSoftware\Brave-Browser",
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
    ),
    (r"Software\Chromium", r"Software\Chromium\NativeMessagingHosts"),
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
        "allowed_origins": EXTENSION_IDS.iter().map(|id| format!("chrome-extension://{id}/")).collect::<Vec<_>>(),
    });
    let dir = crate::store::data_dir();
    std::fs::create_dir_all(&dir)?;
    let manifest_path = dir.join(format!("{HOST_NAME}.json"));
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    let user = RegKey::predef(HKEY_CURRENT_USER);
    for (browser, hosts) in BROWSERS {
        if user.open_subkey(browser).is_ok() {
            let (key, _) = user.create_subkey(format!(r"{hosts}\{HOST_NAME}"))?;
            key.set_value("", &manifest_path.as_os_str())?;
        }
    }
    Ok(())
}

/// Removes what [`register`] added. Run by the uninstaller.
#[cfg(windows)]
pub fn unregister() -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let user = RegKey::predef(HKEY_CURRENT_USER);
    for (_, hosts) in BROWSERS {
        if let Ok(key) = user.open_subkey_with_flags(hosts, winreg::enums::KEY_ALL_ACCESS) {
            let _ = key.delete_subkey_all(HOST_NAME);
        }
    }
    match std::fs::remove_file(crate::store::data_dir().join(format!("{HOST_NAME}.json"))) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
pub fn register() -> io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn unregister() -> io::Result<()> {
    Ok(())
}
