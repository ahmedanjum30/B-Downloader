//! Windows notifications ("toasts"): a download added while the window is in
//! the tray says so, without bringing the window up or taking the focus.
//!
//! A notification needs an app ID Windows knows. The Store version has its
//! package's; the installed one registers its own under
//! HKCU\Software\Classes\AppUserModelId, with the name and icon Windows shows.

/// The installed app's notification ID (the Store version uses its package's).
#[cfg(windows)]
const APP_ID: &str = "AhmadAnjum.BDownloader";

/// Registers the app's notification ID. Not fatal: without it there are just
/// no notifications.
#[cfg(windows)]
pub fn register() {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    if crate::update::is_packaged() {
        return;
    }
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}")) else {
        return;
    };
    let _ = key.set_value("DisplayName", &"B-Downloader");
    // The extension's icon, which the installer puts next to the app.
    if let Some(icon) = crate::browser::extension_dir().map(|dir| dir.join("icons").join("128.png"))
        && icon.exists()
    {
        let _ = key.set_value("IconUri", &icon.to_string_lossy().into_owned());
    }
}

#[cfg(not(windows))]
pub fn register() {}

/// Shows a notification with a `title` line and a `body` line, through
/// AppleScript, which needs no permission set up first.
#[cfg(target_os = "macos")]
pub fn show(title: &str, body: &str) {
    // Passed as arguments, never put in the script, so a file name can't change it.
    const SCRIPT: &str = "on run argv\ndisplay notification (item 2 of argv) with title (item 1 of argv)\nend run";
    let (title, body) = (title.to_owned(), body.to_owned());
    // Waited for on a thread of its own, so it doesn't stay behind as a zombie.
    std::thread::spawn(move || {
        let shown = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", SCRIPT, &title, &body])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if let Err(e) = shown {
            log::warn!("couldn't show a notification: {e}");
        }
    });
}

/// Shows a notification with a `title` line and a `body` line, on a thread of
/// its own: Windows takes a moment, and the window shouldn't wait.
#[cfg(not(target_os = "macos"))]
pub fn show(title: &str, body: &str) {
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    std::thread::spawn(move || {
        if let Err(e) = toast(&xml) {
            log::warn!("couldn't show a notification: {e}");
        }
    });
}

#[cfg(windows)]
fn toast(xml: &str) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let notifier = if crate::update::is_packaged() {
        ToastNotificationManager::CreateToastNotifier()?
    } else {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
    };
    notifier.Show(&toast)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn toast(_xml: &str) -> Result<(), String> {
    Ok(())
}

/// `text` safe to put in the notification's XML.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_escaped() {
        assert_eq!(escape(r#"a&b <c> "d".zip"#), "a&amp;b &lt;c&gt; &quot;d&quot;.zip");
    }
}
