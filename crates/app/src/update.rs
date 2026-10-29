//! Updates from GitHub releases: finds a newer version, and runs its
//! installer once it has been downloaded and checked.

use std::path::{Path, PathBuf};

use bdownloader_core::Engine;

const REPO: &str = "https://github.com/ahmedanjum30/B-Downloader";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The release page, for "What's new".
    pub page: String,
    pub installer_url: String,
    pub installer_name: String,
    /// The installer's SHA-256 from the release's `SHA256SUMS.txt`.
    pub sha256: String,
}

/// The newest release, if it's newer than this copy.
pub async fn check(engine: Engine) -> Result<Option<Release>, String> {
    check_against(engine, env!("CARGO_PKG_VERSION")).await
}

/// Uses the release pages instead of GitHub's API: the API allows only 60
/// requests an hour per internet address without an account, which people
/// sharing an address (common with home providers) run out of.
async fn check_against(engine: Engine, current: &str) -> Result<Option<Release>, String> {
    // `releases/latest` redirects to the newest full release's page, `.../releases/tag/v1.2.3`.
    let page = engine.final_url(&format!("{REPO}/releases/latest")).await.map_err(|e| e.to_string())?;
    let tag = page
        .rsplit_once("/releases/tag/")
        .map(|(_, tag)| tag.trim_end_matches('/'))
        .ok_or("no release found on GitHub")?
        .to_owned();
    let version = tag.trim_start_matches('v').to_owned();
    if !is_newer(&version, current) {
        return Ok(None);
    }
    let installer_name = format!("BDownloader-Setup-{version}.exe");
    let sums = engine
        .get_text(&format!("{REPO}/releases/download/{tag}/SHA256SUMS.txt"), 64 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let sha256 = checksum_for(&sums, &installer_name).ok_or("the installer is missing from the release")?;
    Ok(Some(Release {
        installer_url: format!("{REPO}/releases/download/{tag}/{installer_name}"),
        installer_name,
        version,
        page,
        sha256,
    }))
}

/// Compares dotted version numbers: `0.1.10` is newer than `0.1.9`.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|part| part.parse().ok()).collect() };
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Finds `name` in a `sha256sum`-style file (`<hex>  <file name>` per line).
fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hex.len() == 64).then(|| hex.to_ascii_lowercase())
    })
}

/// The Microsoft Store version (an MSIX package). The Store updates it, so it
/// doesn't look for updates on GitHub.
#[cfg(windows)]
pub fn is_packaged() -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    const APPMODEL_ERROR_NO_PACKAGE: i32 = 15700;
    static PACKAGED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut len = 0;
        // SAFETY: asks only for the length (no buffer), which is allowed.
        unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) as i32 != APPMODEL_ERROR_NO_PACKAGE }
    })
}

#[cfg(not(windows))]
pub fn is_packaged() -> bool {
    false
}

/// Updates only make sense for an installed copy (the uninstaller sits next
/// to it). Development builds would otherwise install a release over nothing.
pub fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("unins000.exe")))
        .is_some_and(|uninstaller| uninstaller.exists())
}

/// Where downloaded installers go.
pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join("BDownloader-update")
}

/// What the installer does with the app afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relaunch {
    /// The user was quitting anyway.
    No,
    Window,
    /// Back in the tray, as it was.
    Tray,
}

/// Starts the installer silently. It closes this app if it's still running,
/// replaces the files and starts the app again as `relaunch` says.
pub fn install(installer: &Path, relaunch: Relaunch) -> std::io::Result<()> {
    // Read by the `if_param = "UPDATE=…"` runs in installer\bdownloader.toml.
    let mode = match relaunch {
        Relaunch::No => "/UPDATE=0",
        Relaunch::Window => "/UPDATE=1",
        Relaunch::Tray => "/UPDATE=2",
    };
    std::process::Command::new(installer)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", mode])
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(!is_newer("0.2.0-beta", "0.1.0"));
    }

    /// Talks to GitHub, so it only runs when asked:
    /// `cargo test -p bdownloader -- --ignored live_release`
    #[tokio::test]
    #[ignore]
    async fn live_release_is_found_and_verified() {
        let engine = Engine::new().unwrap();
        let release = check_against(engine.clone(), "0.0.1").await.unwrap().expect("a newer release");
        println!("{release:?}");
        assert!(release.installer_url.ends_with(".exe"));
        assert_eq!(release.sha256.len(), 64);
        assert_eq!(check_against(engine, "999.0.0").await.unwrap(), None);
    }

    #[test]
    fn checksums() {
        let sums = "aaaa  other.zip\nB6E72874F8CB5C4ED77F9B03D7F1FDE851B9455A7FF02A1E1119C876318EBC65  BDownloader-Setup-0.1.1.exe\n";
        assert_eq!(
            checksum_for(sums, "BDownloader-Setup-0.1.1.exe").as_deref(),
            Some("b6e72874f8cb5c4ed77f9b03d7f1fde851b9455a7ff02a1e1119c876318ebc65")
        );
        assert_eq!(checksum_for(sums, "other.zip"), None);
        assert_eq!(checksum_for(sums, "missing.exe"), None);
    }
}
