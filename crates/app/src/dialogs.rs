//! The dialogs shown over the main window.

use std::path::Path;

use bdownloader_core::MAX_CONNECTIONS;
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, space, text, text_input};
use iced::{Center, Element, Fill, Shrink};

use crate::app::{AddDialog, Message, Probe, RefreshDialog};
use crate::browser;
use crate::format;
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::store::{HashKind, Settings, ThemeChoice};
use crate::style;
use crate::view::{BOLD, SEMIBOLD, colored, file_badge, muted};

fn frame<'a>(title: &'a str, body: Element<'a, Message>, footer: Element<'a, Message>) -> Element<'a, Message> {
    let header = row![
        text(title).size(20).font(BOLD).width(Fill),
        button(icon(&icons::CLOSE, 18.0, |c| c.muted))
            .padding(6)
            .style(style::icon_button)
            .on_press(Message::CloseModal),
    ]
    .align_y(Center);
    let dialog = container(column![header, body, footer].spacing(22))
        .padding(26)
        .width(540)
        .style(style::dialog);
    // Scrolls when the window is too short for it, instead of being cut off.
    container(scrollable(dialog).height(Shrink).spacing(6)).padding(16).into()
}

fn field<'a>(label: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![muted(label).size(13).font(SEMIBOLD), content.into()].spacing(8).into()
}

fn input<'a>(placeholder: &'a str, value: &'a str) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value).padding(12).size(14).style(style::input)
}

fn folder_picker(dir: &Path) -> Element<'_, Message> {
    row![
        container(text(format::middle_ellipsis(&dir.display().to_string(), 52)).size(14))
            .padding(12)
            .width(Fill)
            .style(style::panel),
        button(text("Browse").size(14))
            .padding([11, 16])
            .style(style::secondary)
            .on_press(Message::BrowseFolder),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

/// Number of connections: a slider for big changes, buttons for exact ones.
fn stepper<'a>(value: usize, on_change: fn(usize) -> Message) -> Element<'a, Message> {
    let step = |handle, enabled: bool, next: usize| {
        button(icon(handle, 15.0, |c| c.text))
            .padding(10)
            .style(style::secondary)
            .on_press_maybe(enabled.then(|| on_change(next)))
    };
    row![
        step(&icons::MINUS, value > 1, value.saturating_sub(1)),
        container(text(value.to_string()).size(15).font(SEMIBOLD)).center_x(44),
        step(&icons::PLUS, value < MAX_CONNECTIONS, value + 1),
        slider(1..=MAX_CONNECTIONS as u32, value as u32, move |n| on_change(n as usize)).width(Fill),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn footer<'a>(confirm: &'a str, on_confirm: Option<Message>) -> Element<'a, Message> {
    row![
        space().width(Fill),
        button(text("Cancel").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
        button(text(confirm).size(14).font(SEMIBOLD))
            .padding([10, 20])
            .style(style::primary)
            .on_press_maybe(on_confirm),
    ]
    .spacing(10)
    .into()
}

/// `existing`: an unfinished download of the same file (its id and how much
/// of it is done), which this link can continue instead of starting over.
pub fn add(dialog: &AddDialog, existing: Option<(u64, u64)>) -> Element<'_, Message> {
    let url = input("https://example.com/file.zip", &dialog.url)
        .on_input(Message::AddUrl)
        .on_paste(Message::AddUrlPasted)
        .on_submit(Message::AddUrlSubmit);

    let mut body = column![field("Link", url)].spacing(18);

    let probe: Option<Element<'_, Message>> = if let Some(source) = &dialog.youtube {
        let total: Option<u64> = source.video.iter().chain(&source.audio).map(|f| f.content_length).sum();
        let size = total.map_or("YouTube video".to_owned(), |t| format!("YouTube video · {}", format::size(t)));
        let (badge, support) = if source.video.is_some() {
            ("MP4", "Picture and sound joined into one file · can resume")
        } else {
            ("M4A", "Sound only · can resume")
        };
        Some(summary(badge.to_owned(), size, support, |c| c.success))
    } else {
        match &dialog.probe {
            Probe::Idle => None,
            Probe::Checking => Some(muted("Checking link…").size(13).into()),
            Probe::Failed(error) => Some(colored(error.as_str(), |c| c.danger).size(13).into()),
            Probe::Ready(info) => {
                let name = if dialog.file_name.is_empty() {
                    "download"
                } else {
                    dialog.file_name.as_str()
                };
                let extension = name
                    .rsplit_once('.')
                    .filter(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()))
                    .map_or("FILE".to_owned(), |(_, ext)| ext.to_ascii_uppercase());
                let (extension, size) = if info.is_stream() {
                    ("VIDEO".to_owned(), "Video stream".to_owned())
                } else {
                    (extension, info.size.map_or("Unknown size".to_owned(), format::size))
                };
                let (support, pick): (&str, style::Pick) = if info.is_stream() {
                    ("Downloads in parts · can resume", |c| c.success)
                } else if info.is_web_page() {
                    ("This link opens a web page, not a file", |c| c.warning)
                } else if info.resumable {
                    ("Multiple connections · can resume", |c| c.success)
                } else {
                    ("Single connection · can't resume", |c| c.warning)
                };
                Some(summary(extension, size, support, pick))
            }
        }
    };
    if let Some(probe) = probe {
        body = body.push(probe);
    }
    if let Some((id, done)) = existing {
        body = body.push(
            container(
                row![
                    icon(&icons::LINK, 18.0, |c| c.accent),
                    text(format!("You already have {} of this file in an unfinished download.", format::size(done)))
                        .size(14)
                        .width(Fill),
                    button(text("Continue that download").size(13).font(SEMIBOLD))
                        .padding([7, 14])
                        .style(style::primary)
                        .on_press(Message::ContinueExisting(id)),
                ]
                .spacing(12)
                .align_y(Center),
            )
            .padding(12)
            .width(Fill)
            .style(style::accent_soft(10.0)),
        );
    }

    body = body.push(field(
        "Save as",
        input("Name from the server", &dialog.file_name).on_input(Message::AddName),
    ));
    body = body.push(field("Save to", folder_picker(&dialog.dir)));
    body = body.push(row![field("Connections", stepper(dialog.connections, Message::AddConnections))]);

    let verify = checkbox(dialog.verify)
        .label("Verify checksum when finished")
        .on_toggle(Message::AddVerify)
        .size(18)
        .text_size(14)
        .spacing(10);
    let mut checksum = column![row![icon(&icons::SHIELD, 18.0, |c| c.muted), verify].spacing(10).align_y(Center)].spacing(10);
    if dialog.verify {
        checksum = checksum.push(
            row![
                pick_list(&HashKind::ALL[..], Some(dialog.hash_kind), Message::AddHashKind)
                    .padding(11)
                    .text_size(14)
                    .width(120),
                input("Expected checksum (hex)", &dialog.hash).on_input(Message::AddHash),
            ]
            .spacing(8),
        );
    }
    body = body.push(checksum);

    frame(
        "Add download",
        body.into(),
        footer("Download", dialog.can_submit().then_some(Message::ConfirmAdd)),
    )
}

fn updates<'a>(draft: &Settings, status: String) -> Element<'a, Message> {
    let mode = row([(true, "Install automatically"), (false, "Ask me first")].map(|(auto, label)| {
        button(text(label).size(14).width(Fill).center())
            .padding([8, 12])
            .width(Fill)
            .style(style::nav(draft.auto_update == auto))
            .on_press(Message::SettingsAutoUpdate(auto))
            .into()
    }))
    .spacing(4);
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    let status = if status.is_empty() { version } else { format!("{version} · {status}") };
    column![
        container(mode).padding(4).style(style::panel),
        row![
            muted(status).size(13).width(Fill),
            button(text("Check now").size(13))
                .padding([7, 14])
                .style(style::secondary)
                .on_press(Message::CheckForUpdates { manual: true }),
        ]
        .spacing(12)
        .align_y(Center),
        muted("Automatic updates install while nothing is downloading and BDownloader is in the tray.").size(12),
        row![
            muted("Something not working? The log helps find out why.").size(12).width(Fill),
            button(text("Open log folder").size(13))
                .padding([7, 14])
                .style(style::secondary)
                .on_press(Message::OpenLogs),
        ]
        .spacing(12)
        .align_y(Center),
    ]
    .spacing(8)
    .into()
}

pub fn settings(draft: &Settings, update_status: String) -> Element<'_, Message> {
    let themes = row([
        (ThemeChoice::System, "System"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::Light, "Light"),
    ]
    .map(|(choice, label)| {
        button(text(label).size(14).width(Fill).center())
            .padding([8, 12])
            .width(Fill)
            .style(style::nav(draft.theme == choice))
            .on_press(Message::SettingsTheme(choice))
            .into()
    }))
    .spacing(4);

    let on_close = row([(true, "Keep running in the tray"), (false, "Exit BDownloader")].map(|(to_tray, label)| {
        button(text(label).size(14).width(Fill).center())
            .padding([8, 12])
            .width(Fill)
            .style(style::nav(draft.close_to_tray == to_tray))
            .on_press(Message::SettingsCloseToTray(to_tray))
            .into()
    }))
    .spacing(4);

    let body = column![
        field("Default download folder", folder_picker(&draft.download_dir)),
        field("Connections per download", stepper(draft.connections, Message::SettingsConnections)),
        field("Theme", container(themes).padding(4).style(style::panel)),
        field("When I close the window", container(on_close).padding(4).style(style::panel)),
        field("Updates", updates(draft, update_status)),
    ]
    .spacing(18);

    frame("Settings", body.into(), footer("Save", Some(Message::SaveSettings)))
}

/// A value to paste somewhere else, with a button that copies it.
fn copy_field<'a>(value: String, copied: bool) -> Element<'a, Message> {
    let label: Element<'a, Message> = if copied {
        row![
            icon(&icons::CHECK, 15.0, |c| c.success),
            colored("Copied", |c| c.success).size(13).font(SEMIBOLD)
        ]
        .spacing(6)
        .align_y(Center)
        .into()
    } else {
        row![icon(&icons::COPY, 15.0, |c| c.muted), muted("Copy").size(13).font(SEMIBOLD)]
            .spacing(6)
            .align_y(Center)
            .into()
    };
    // The whole field is the button, so a click anywhere on it copies.
    button(
        row![text(value.clone()).size(14).font(SEMIBOLD).width(Fill), label]
            .spacing(12)
            .align_y(Center),
    )
    .padding([10, 14])
    .width(Fill)
    .style(style::secondary)
    .on_press(Message::Copy(value))
    .into()
}

pub fn extension<'a>(connected: bool, copied: Option<&str>) -> Element<'a, Message> {
    let (title, detail, pick): (&str, &str, style::Pick) = if connected {
        ("Connected", "Downloads from your browser come to BDownloader.", |c| c.success)
    } else {
        ("Not connected", "Add the extension to Chrome, Edge or Brave.", |c| c.muted)
    };
    let status = container(
        row![
            container(icon(&icons::GLOBE, 22.0, |c| c.accent))
                .center(44)
                .style(style::accent_soft(12.0)),
            column![colored(title, pick).size(15).font(SEMIBOLD), muted(detail).size(13)].spacing(3),
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(14)
    .width(Fill)
    .style(style::panel);

    let step = |n: &'a str, line: &'a str| {
        row![
            container(colored(n, |c| c.accent).size(12).font(BOLD))
                .center(24)
                .style(style::accent_soft(12.0)),
            text(line).size(14),
        ]
        .spacing(12)
        .align_y(Center)
    };
    let page = browser::EXTENSIONS_PAGE.to_owned();
    let folder = browser::extension_dir().map_or("the extension folder next to BDownloader.exe".to_owned(), |dir| dir.display().to_string());
    let page_copied = copied == Some(page.as_str());
    let folder_copied = copied == Some(folder.as_str());

    let body = column![
        status,
        muted("What it does").size(13).font(SEMIBOLD),
        text("• Downloads you start in the browser go to BDownloader.").size(14),
        text("• Right-click a link and choose Download with BDownloader.").size(14),
        muted("How to add it").size(13).font(SEMIBOLD),
        step("1", "Copy this address and paste it into the browser's address bar:"),
        copy_field(page, page_copied),
        step("2", "Turn on Developer mode (top right)."),
        step("3", "Click Load unpacked and choose this folder:"),
        copy_field(folder, folder_copied),
        muted("It shows as connected here once the browser has talked to BDownloader.").size(13),
    ]
    .spacing(12);

    let footer = row![
        space().width(Fill),
        button(text("Close").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
    ];

    frame("Browser extension", body.into(), footer.into())
}

pub fn refresh(dialog: &RefreshDialog) -> Element<'_, Message> {
    let size = dialog.size.map_or("Unknown size".to_owned(), format::size);
    let extension = dialog
        .name
        .rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()))
        .map_or("FILE".to_owned(), |(_, ext)| ext.to_ascii_uppercase());
    let file = container(
        row![
            file_badge(extension),
            column![
                text(format::middle_ellipsis(&dialog.name, 48)).size(15).font(SEMIBOLD),
                muted(size).size(13)
            ]
            .spacing(3)
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(12)
    .width(Fill)
    .style(style::panel);

    let mut body =
        column![
        file,
        text("Download links often stop working after a while. Give BDownloader a new link to the same file, and it continues from where it stopped.")
            .size(14),
    ]
        .spacing(16);

    if dialog.referrer.is_some() {
        body = body.push(
            row![
                button(text("Open download page").size(14))
                    .padding([10, 16])
                    .style(style::secondary)
                    .on_press(Message::RefreshFromPage),
                muted("Then click the download again in your browser. BDownloader picks up the new link.").size(13),
            ]
            .spacing(12)
            .align_y(Center),
        );
    }

    body = body.push(field(
        if dialog.referrer.is_some() {
            "Or paste the new link"
        } else {
            "Paste the new link, or click the download again in your browser"
        },
        input("https://…", &dialog.url)
            .on_input(Message::RefreshUrl)
            .on_paste(Message::RefreshPasted),
    ));

    let status: Option<Element<'_, Message>> = match &dialog.probe {
        Probe::Idle => None,
        Probe::Checking => Some(muted("Checking the link…").size(13).into()),
        Probe::Failed(error) => Some(colored(error.as_str(), |c| c.danger).size(13).into()),
        Probe::Ready(_) if dialog.matches() => Some(
            row![
                icon(&icons::CHECK, 16.0, |c| c.success),
                colored("Same file. Ready to continue.", |c| c.success).size(13)
            ]
            .spacing(8)
            .align_y(Center)
            .into(),
        ),
        Probe::Ready(info) if !info.resumable => Some(
            colored("This server can't continue a partial download, so this link can't be used.", |c| {
                c.warning
            })
            .size(13)
            .into(),
        ),
        Probe::Ready(info) => Some(
            colored(
                format!(
                    "This link is for a different file ({} instead of {}).",
                    info.size.map_or("unknown size".to_owned(), format::size),
                    dialog.size.map_or("unknown size".to_owned(), format::size)
                ),
                |c| c.warning,
            )
            .size(13)
            .into(),
        ),
    };
    if let Some(status) = status {
        body = body.push(status);
    }

    frame(
        "Refresh download link",
        body.into(),
        footer("Continue download", dialog.matches().then_some(Message::ConfirmRefresh)),
    )
}

pub fn remove(item: &Item, delete_file: bool) -> Element<'_, Message> {
    let finished = item.state == State::Completed;
    let mut body = column![
        container(
            row![
                file_badge(item.extension()),
                text(format::middle_ellipsis(&item.display_name(), 48)).size(15).font(SEMIBOLD),
            ]
            .spacing(14)
            .align_y(Center),
        )
        .padding(12)
        .width(Fill)
        .style(style::panel),
    ]
    .spacing(16);

    if finished {
        body = body.push(text("It's removed from the list. The file itself stays where it is, unless you tick the box.").size(14));
        body = body.push(
            checkbox(delete_file)
                .label("Also delete the file from disk (moves it to the Recycle Bin)")
                .on_toggle(Message::RemoveDeleteFile)
                .size(18)
                .text_size(14)
                .spacing(10),
        );
    } else {
        let done = if item.downloaded > 0 {
            format!("The {} downloaded so far will be deleted.", format::size(item.downloaded))
        } else {
            "Nothing has been downloaded yet.".to_owned()
        };
        body = body.push(text(format!("The download stops and is removed from the list. {done}")).size(14));
    }

    let footer = row![
        space().width(Fill),
        button(text("Cancel").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
        button(
            text(if finished && !delete_file { "Remove" } else { "Remove and delete" })
                .size(14)
                .font(SEMIBOLD)
        )
        .padding([10, 20])
        .style(style::danger)
        .on_press(Message::ConfirmRemove),
    ]
    .spacing(10);

    frame("Remove download", body.into(), footer.into())
}

/// What's about to be downloaded: a file-type badge, the size and how it downloads.
fn summary<'a>(badge: String, size: String, support: &'a str, pick: style::Pick) -> Element<'a, Message> {
    container(
        row![
            file_badge(badge),
            column![text(size).size(15).font(SEMIBOLD), colored(support, pick).size(13)].spacing(3)
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(12)
    .width(Fill)
    .style(style::panel)
    .into()
}
