//! Report a problem: a new GitHub issue, filled in with what the user wrote,
//! the app and Windows versions, and the end of the log. The browser shows
//! it before it's sent, so the user sees everything that goes public.

use iced::Task;
use iced::widget::text_editor;

use super::{App, Message, Modal};
use crate::store;

const NEW_ISSUE: &str = "https://github.com/ahmedanjum30/BDownloader/issues/new";
/// GitHub refuses longer links to a new issue, so the log is shortened to fit.
const MAX_URL: usize = 7_500;
/// The most of the log that goes in, before shortening to fit.
const LOG_LINES: usize = 150;

pub struct ReportDialog {
    pub description: text_editor::Content,
    pub include_log: bool,
}

impl App {
    pub(super) fn handle_report(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenReport => {
                // Settings, where it's opened from, comes back afterwards.
                if let Some(open) = self.modal.take() {
                    self.covered.push(open);
                }
                self.modal = Some(Modal::Report(Box::new(ReportDialog {
                    description: text_editor::Content::new(),
                    include_log: true,
                })));
            }
            Message::ReportEdit(action) => {
                if let Some(Modal::Report(dialog)) = &mut self.modal {
                    dialog.description.perform(action);
                }
            }
            Message::ReportIncludeLog(include) => {
                if let Some(Modal::Report(dialog)) = &mut self.modal {
                    dialog.include_log = include;
                }
            }
            Message::SendReport => {
                if let Some(Modal::Report(dialog)) = self.modal.take() {
                    let log = dialog.include_log.then(recent_log);
                    let url = issue_url(dialog.description.text().trim(), &system_info(), log.as_deref());
                    log::info!("opening a problem report on GitHub");
                    store::open_file(std::path::Path::new(&url));
                }
            }
            _ => {}
        }
        Task::none()
    }
}

/// The new-issue link: title from the first line, then the description, the
/// versions, and as much of the log's end as fits.
fn issue_url(description: &str, system: &str, log: Option<&str>) -> String {
    let first_line = description.lines().next().unwrap_or_default().trim();
    let title = match first_line.chars().count() {
        0 => "Problem report".to_owned(),
        n if n > 80 => format!("{}…", first_line.chars().take(79).collect::<String>()),
        _ => first_line.to_owned(),
    };
    let body_with = |log: Option<&str>| {
        let mut body = String::new();
        body += if description.is_empty() { "_(No description.)_" } else { description };
        body += &format!("\n\n---\n{system}\n");
        if let Some(log) = log {
            body += &format!("\n<details><summary>Log (end)</summary>\n\n```\n{log}\n```\n</details>\n");
        }
        body
    };
    let url = |body: &str| format!("{NEW_ISSUE}?title={}&body={}", encode(&title), encode(body));

    let Some(log) = log else { return url(&body_with(None)) };
    // Drop the oldest lines until it fits.
    let lines: Vec<&str> = log.lines().collect();
    for skip in 0..lines.len() {
        let candidate = url(&body_with(Some(&lines[skip..].join("\n"))));
        if candidate.len() <= MAX_URL {
            return candidate;
        }
    }
    url(&body_with(None))
}

/// The end of the log, with the user's own folder name taken out.
fn recent_log() -> String {
    let path = store::data_dir().join("logs").join("bdownloader.log");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(LOG_LINES)..].join("\n");
    match std::env::var("USERPROFILE") {
        Ok(home) if !home.is_empty() => tail.replace(&home, "%USERPROFILE%"),
        _ => tail,
    }
}

fn system_info() -> String {
    let install = if crate::update::is_packaged() {
        "Microsoft Store"
    } else if crate::update::is_installed() {
        "installer"
    } else {
        "not installed"
    };
    format!(
        "BDownloader {} ({install}) on {}",
        env!("CARGO_PKG_VERSION"),
        windows_version().unwrap_or_else(|| "Windows".to_owned())
    )
}

/// "Windows 11 24H2 (build 26100)", from the registry.
#[cfg(windows)]
fn windows_version() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let build: String = key.get_value("CurrentBuild").ok()?;
    let release: String = key.get_value("DisplayVersion").unwrap_or_default();
    // Windows 11 still says "Windows 10" in ProductName; the build tells them apart.
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22_000) {
        "Windows 11"
    } else {
        "Windows 10"
    };
    Some(format!("{name} {release} (build {build})").replace("  ", " "))
}

#[cfg(not(windows))]
fn windows_version() -> Option<String> {
    None
}

/// Percent-encodes everything but letters, digits and `-_.~`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out += &format!("%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_the_first_line() {
        let url = issue_url("Firefox download hangs\nIt stops at 50%.", "BDownloader 1.1.0", None);
        assert!(url.starts_with(&format!("{NEW_ISSUE}?title=Firefox%20download%20hangs&body=")));
        assert!(url.contains("It%20stops%20at%2050%25."));
        assert!(url.contains("BDownloader%201.1.0"));
    }

    #[test]
    fn a_long_log_is_cut_from_the_start_to_fit() {
        let log: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let url = issue_url("Slow", "BDownloader 1.1.0", Some(&log));
        assert!(url.len() <= MAX_URL, "{}", url.len());
        assert!(url.contains("line%201999"), "the newest lines are kept");
        assert!(!url.contains("line%200%0A"), "the oldest are dropped");
    }

    #[test]
    fn encoding_keeps_only_safe_characters() {
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
