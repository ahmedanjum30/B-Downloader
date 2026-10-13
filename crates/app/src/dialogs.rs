//! The dialogs shown over the main window.

use std::path::Path;

use bdownloader_core::MAX_CONNECTIONS;
use iced::widget::{button, checkbox, column, container, pick_list, row, space, text, text_input};
use iced::{Center, Element, Fill};

use crate::app::{AddDialog, Message, Probe};
use crate::browser;
use crate::format;
use crate::icons::{self, icon};
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
    container(column![header, body, footer].spacing(22))
        .padding(26)
        .width(540)
        .style(style::dialog)
        .into()
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
    ]
    .spacing(4)
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

pub fn add(dialog: &AddDialog) -> Element<'_, Message> {
    let url = input("https://example.com/file.zip", &dialog.url)
        .on_input(Message::AddUrl)
        .on_paste(Message::AddUrlPasted)
        .on_submit(Message::AddUrlSubmit);

    let mut body = column![field("Link", url)].spacing(18);

    let probe: Option<Element<'_, Message>> = match &dialog.probe {
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
            let size = info.size.map_or("Unknown size".to_owned(), format::size);
            let (support, pick): (&str, style::Pick) = if info.is_web_page() {
                ("This link opens a web page, not a file", |c| c.warning)
            } else if info.resumable {
                ("Multiple connections · can resume", |c| c.success)
            } else {
                ("Single connection · can't resume", |c| c.warning)
            };
            Some(
                container(
                    row![
                        file_badge(extension),
                        column![text(size).size(15).font(SEMIBOLD), colored(support, pick).size(13)].spacing(3)
                    ]
                    .spacing(14)
                    .align_y(Center),
                )
                .padding(12)
                .width(Fill)
                .style(style::panel)
                .into(),
            )
        }
    };
    if let Some(probe) = probe {
        body = body.push(probe);
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

pub fn settings(draft: &Settings) -> Element<'_, Message> {
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
    ]
    .spacing(18);

    frame("Settings", body.into(), footer("Save", Some(Message::SaveSettings)))
}

pub fn extension<'a>(connected: bool) -> Element<'a, Message> {
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

    let step = |n: &'a str, line: Element<'a, Message>| {
        row![
            container(colored(n, |c| c.accent).size(12).font(BOLD))
                .center(24)
                .style(style::accent_soft(12.0)),
            line,
        ]
        .spacing(12)
        .align_y(Center)
    };
    let folder = browser::extension_dir().map_or("the extension folder next to BDownloader.exe".to_owned(), |dir| dir.display().to_string());

    let body = column![
        status,
        muted("What it does").size(13).font(SEMIBOLD),
        text("• Downloads you start in the browser go to BDownloader.").size(14),
        text("• Right-click a link and choose Download with BDownloader.").size(14),
        muted("How to add it").size(13).font(SEMIBOLD),
        step(
            "1",
            row![
                text("In the browser's address bar, open ").size(14),
                text("chrome://extensions").size(14).font(SEMIBOLD)
            ]
            .into()
        ),
        step("2", text("Turn on Developer mode (top right).").size(14).into()),
        step("3", text("Click Load unpacked and choose this folder:").size(14).into()),
        container(text(folder).size(13).font(SEMIBOLD))
            .padding(12)
            .width(Fill)
            .style(style::panel),
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
