//! The dialogs shown over the main window.

use std::path::Path;

use bdownloader_core::MAX_CONNECTIONS;
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, slider, space, text, text_editor, text_input};
use iced::{Center, Element, Fill, Shrink};

use crate::app::{AddDialog, MAX_AT_ONCE, Message, Probe, RefreshDialog, ReportDialog, SettingsTab, WhenDone};
use crate::browser::Browser;
use crate::format;
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::store::{self, HashKind, Settings, ThemeChoice};
use crate::style;
use crate::view::{BOLD, SEMIBOLD, colored, file_badge, muted};

fn frame<'a>(title: &'a str, body: Element<'a, Message>, footer: Element<'a, Message>) -> Element<'a, Message> {
    sized_frame(title, body, footer, 540.0)
}

fn sized_frame<'a>(title: &'a str, body: Element<'a, Message>, footer: Element<'a, Message>, width: f32) -> Element<'a, Message> {
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
        .width(width)
        .style(style::dialog);
    // Scrolls when the window is too short for it, instead of being cut off.
    container(scrollable(dialog).height(Shrink).spacing(6).style(style::scrollbar))
        .padding(16)
        .into()
}

fn field<'a>(label: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![muted(label).size(13).font(SEMIBOLD), content.into()].spacing(8).into()
}

fn input<'a>(placeholder: &'a str, value: &'a str) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value).padding(12).size(14).style(style::input)
}

fn folder_picker<'a>(dir: &Path) -> Element<'a, Message> {
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

/// A number from 1 to `max`: a slider for big changes, buttons for exact ones.
fn stepper<'a>(value: usize, max: usize, on_change: fn(usize) -> Message) -> Element<'a, Message> {
    let step = |handle, enabled: bool, next: usize| {
        button(icon(handle, 15.0, |c| c.text))
            .padding(10)
            .style(style::secondary)
            .on_press_maybe(enabled.then(|| on_change(next)))
    };
    row![
        step(&icons::MINUS, value > 1, value.saturating_sub(1)),
        container(text(value.to_string()).size(15).font(SEMIBOLD)).center_x(44),
        step(&icons::PLUS, value < max, value + 1),
        slider(1..=max as u32, value as u32, move |n| on_change(n as usize)).width(Fill),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn footer<'a>(confirm: &'a str, on_confirm: Option<Message>) -> Element<'a, Message> {
    row![
        space().width(Fill),
        cancel(),
        button(text(confirm).size(14).font(SEMIBOLD))
            .padding([9, 16])
            .style(style::primary)
            .on_press_maybe(on_confirm),
    ]
    .spacing(8)
    .into()
}

fn cancel<'a>() -> Element<'a, Message> {
    button(text("Cancel").size(14).font(SEMIBOLD))
        .padding([9, 14])
        .style(style::ghost)
        .on_press(Message::CloseModal)
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
    body = body.push(field("Save to", folder_picker(&dialog.save_dir())));
    body = body.push(row![field(
        "Connections",
        stepper(dialog.connections, MAX_CONNECTIONS, Message::AddConnections)
    )]);

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

    let footer = row![
        space().width(Fill),
        cancel(),
        button(text("Download later").size(14).font(SEMIBOLD))
            .padding([9, 14])
            .style(style::secondary)
            .on_press_maybe(dialog.can_submit().then_some(Message::ConfirmAddLater)),
        button(text("Download").size(14).font(SEMIBOLD))
            .padding([10, 20])
            .style(style::primary)
            .on_press_maybe(dialog.can_submit().then_some(Message::ConfirmAdd)),
    ]
    .spacing(10);
    frame("Add download", body.into(), footer.into())
}

/// A line explaining the setting above it.
fn hint<'a>(content: &'a str) -> Element<'a, Message> {
    muted(content).size(12).into()
}

fn choices<'a, T: Copy + PartialEq>(options: &[(T, &'a str)], current: T, on_press: fn(T) -> Message) -> Element<'a, Message> {
    let buttons = row(options.iter().map(|&(value, label)| {
        button(text(label).size(14).width(Fill).center())
            .padding([8, 12])
            .width(Fill)
            .style(style::tab(current == value))
            .on_press(on_press(value))
            .into()
    }))
    .spacing(4);
    container(buttons).padding(4).style(style::panel).into()
}

fn updates<'a>(draft: &Settings, status: String) -> Element<'a, Message> {
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    if crate::update::is_packaged() {
        return column![muted(format!("{version} · Updates come from the Microsoft Store.")).size(13)].into();
    }
    let status = if status.is_empty() { version } else { format!("{version} · {status}") };
    column![
        field(
            "New versions",
            column![
                choices(
                    &[(true, "Install automatically"), (false, "Ask me first")],
                    draft.auto_update,
                    Message::SettingsAutoUpdate
                ),
                hint("Automatic updates install while nothing is downloading and no dialog is open."),
            ]
            .spacing(8)
        ),
        row![
            muted(status).size(13).width(Fill),
            button(text("Check now").size(13))
                .padding([7, 14])
                .style(style::secondary)
                .on_press(Message::CheckForUpdates { manual: true }),
        ]
        .spacing(12)
        .align_y(Center),
    ]
    .spacing(18)
    .into()
}

fn help<'a>() -> Element<'a, Message> {
    column![
        field(
            "Something not working?",
            column![
                hint(
                    "Tell us what happened. It opens a report on GitHub, filled in with your app and Windows \
                     versions and BDownloader's log, for you to check before sending."
                ),
                row![
                    button(text("Report a problem").size(14).font(SEMIBOLD))
                        .padding([9, 16])
                        .style(style::primary)
                        .on_press(Message::OpenReport),
                ],
            ]
            .spacing(10)
        ),
        field(
            "Log",
            column![
                hint("A record of connection errors, retries and updates. Links are kept without their tokens, and cookies never."),
                row![
                    button(text("Open log folder").size(14))
                        .padding([9, 16])
                        .style(style::secondary)
                        .on_press(Message::OpenLogs),
                ],
            ]
            .spacing(10)
        ),
        field(
            "Questions and ideas",
            hint("Ask in Discussions on GitHub: github.com/ahmedanjum30/BDownloader/discussions"),
        ),
    ]
    .spacing(22)
    .into()
}

/// How many of the queue's downloads run at once, and when it starts and stops by itself.
fn queue_settings(draft: &Settings) -> Element<'_, Message> {
    let time = |enabled: bool, label: &'static str, value: &str, on_toggle: fn(bool) -> Message, on_input: fn(String) -> Message| {
        let valid = store::parse_time(value).is_some();
        let mut line = row![
            checkbox(enabled).label(label).on_toggle(on_toggle).size(18).text_size(14).spacing(10),
            text_input("HH:MM", value)
                .on_input(on_input)
                .padding([6, 8])
                .size(14)
                .width(70)
                .style(style::input),
        ]
        .spacing(10)
        .align_y(Center);
        if enabled && !valid {
            line = line.push(colored("Use 24-hour time, like 01:30", |c| c.danger).size(12));
        }
        line
    };
    column![
        hint("Downloads you add with Download later wait in the queue, under Queue in the sidebar."),
        field(
            "Downloads at the same time",
            column![
                stepper(draft.queue_at_once, MAX_AT_ONCE, Message::SettingsQueueAtOnce),
                hint("The queue starts the next download as each one finishes."),
            ]
            .spacing(8)
        ),
        field(
            "Schedule",
            column![
                time(
                    draft.queue_start_daily,
                    "Start the queue every day at",
                    &draft.queue_start,
                    Message::SettingsQueueStartDaily,
                    Message::SettingsQueueStart,
                ),
                time(
                    draft.queue_stop_daily,
                    "Stop it every day at",
                    &draft.queue_stop,
                    Message::SettingsQueueStopDaily,
                    Message::SettingsQueueStop,
                ),
                hint("24-hour time. BDownloader has to be running then; it can be in the tray."),
            ]
            .spacing(10)
        ),
    ]
    .spacing(22)
    .into()
}

fn general(draft: &Settings) -> Element<'_, Message> {
    let sort = checkbox(draft.sort_by_type)
        .label("Sort downloads into folders by type")
        .on_toggle(Message::SettingsSortByType)
        .size(18)
        .text_size(14)
        .spacing(10);
    column![
        field(
            "Download folder",
            column![
                folder_picker(&draft.download_dir),
                sort,
                hint("Compressed, Documents, Music, Programs and Video folders inside it. A folder you pick for a download is used as it is."),
            ]
            .spacing(10)
        ),
        field(
            "Theme",
            choices(
                &[
                    (ThemeChoice::System, "System"),
                    (ThemeChoice::Dark, "Dark"),
                    (ThemeChoice::Light, "Light")
                ],
                draft.theme,
                Message::SettingsTheme
            )
        ),
        field(
            "When I close the window",
            column![
                choices(
                    &[(true, "Keep running in the tray"), (false, "Exit BDownloader")],
                    draft.close_to_tray,
                    Message::SettingsCloseToTray
                ),
                hint("In the tray, BDownloader still takes downloads from the browser and runs the queue's schedule."),
            ]
            .spacing(8)
        ),
    ]
    .spacing(22)
    .into()
}

fn downloads(draft: &Settings) -> Element<'_, Message> {
    column![
        field(
            "Connections per download",
            column![
                stepper(draft.connections, MAX_CONNECTIONS, Message::SettingsConnections),
                hint(
                    "Each file downloads in parts over up to this many connections, adding them while the server keeps up. \
                     Sites that allow fewer get fewer."
                ),
            ]
            .spacing(8)
        ),
        field(
            "Speed limits",
            hint("For all downloads together: the gauge button in the top bar. For one download: its details (the i button)."),
        ),
    ]
    .spacing(22)
    .into()
}

pub fn settings(draft: &Settings, update_status: String, tab: SettingsTab) -> Element<'_, Message> {
    let tabs = column(SettingsTab::ALL.map(|t| {
        button(text(t.label()).size(14).font(if t == tab { SEMIBOLD } else { crate::view::FONT }))
            .padding([8, 10])
            .width(Fill)
            .style(style::nav(t == tab))
            .on_press(Message::SettingsTab(t))
            .into()
    }))
    .spacing(2)
    .width(140);
    let (content, about) = match tab {
        SettingsTab::General => (general(draft), "Where downloads go and how BDownloader looks."),
        SettingsTab::Downloads => (downloads(draft), "How each download uses your connection."),
        SettingsTab::Queue => (queue_settings(draft), "Downloads saved for later, and when they run."),
        SettingsTab::Updates => (updates(draft, update_status), "How BDownloader stays up to date."),
        SettingsTab::Help => (help(), "Report a problem, or find the log."),
    };
    let section = column![
        column![text(tab.label()).size(18).font(SEMIBOLD), muted(about).size(13)].spacing(2),
        container(space()).width(Fill).height(1).style(style::rule),
        content,
    ]
    .spacing(18);
    let body = row![tabs, container(section).width(Fill)].spacing(26);
    sized_frame("Settings", body.into(), footer("Save changes", Some(Message::SaveSettings)), 760.0)
}

/// Report a problem: what happened, and whether the log goes along.
pub fn report(dialog: &ReportDialog) -> Element<'_, Message> {
    let body = column![
        field(
            "What went wrong?",
            column![
                text_editor(&dialog.description)
                    .placeholder("For example: the download from this site stops at 50% and says the link expired.")
                    .on_action(Message::ReportEdit)
                    .height(140)
                    .padding(12)
                    .size(14),
                hint("The first line becomes the report's title."),
            ]
            .spacing(8)
        ),
        checkbox(dialog.include_log)
            .label("Include BDownloader's log (the latest part)")
            .on_toggle(Message::ReportIncludeLog)
            .size(18)
            .text_size(14)
            .spacing(10),
        hint(
            "It opens on GitHub in your browser, with your app and Windows versions added. Reports are public: \
             check it there, then click Submit. Your Windows user name is taken out of the log; you need a GitHub account."
        ),
    ]
    .spacing(16);
    let can_send = !dialog.description.text().trim().is_empty();
    frame(
        "Report a problem",
        body.into(),
        footer("Open on GitHub", can_send.then_some(Message::SendReport)),
    )
}

/// The downloads are done; the PC sleeps or shuts down unless cancelled.
pub fn power_countdown<'a>(action: WhenDone, seconds: u64) -> Element<'a, Message> {
    let (title, verb, now) = match action {
        WhenDone::Sleep => ("Going to sleep", "go to sleep", "Sleep now"),
        _ => ("Shutting down", "shut down", "Shut down now"),
    };
    let body = column![
        row![
            icon(&icons::POWER, 20.0, |c| c.accent),
            text(format!("Downloads are done. Your PC will {verb} in {seconds} seconds."))
                .size(15)
                .width(Fill),
        ]
        .spacing(12)
        .align_y(Center),
        muted("Cancel to keep it on.").size(13),
    ]
    .spacing(10);
    frame(title, body.into(), footer(now, Some(Message::PowerNow)))
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
    // The whole field is the button, so a click anywhere on it copies. Long
    // values (links with tokens) are shortened in the middle to fit on one
    // line; the copy is whole.
    button(
        row![
            text(format::middle_ellipsis(&value, 56))
                .size(14)
                .font(SEMIBOLD)
                .wrapping(text::Wrapping::None)
                .width(Fill),
            label
        ]
        .spacing(12)
        .align_y(Center),
    )
    .padding([10, 14])
    .width(Fill)
    .style(style::secondary)
    .on_press(Message::Copy(value))
    .into()
}

pub fn extension<'a>(browser: Browser, connected: bool, copied: Option<&str>) -> Element<'a, Message> {
    let (title, detail, pick): (&str, &str, style::Pick) = if connected {
        ("Connected", "Downloads from your browser come to BDownloader.", |c| c.success)
    } else {
        ("Not connected", "Add the extension to your browser.", |c| c.muted)
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

    // One tab per browser; the steps below are for the chosen one.
    let tabs = row(Browser::ALL.map(|b| {
        button(text(b.name()).size(13).width(Fill).center())
            .padding([7, 4])
            .width(Fill)
            .style(style::tab(b == browser))
            .on_press(Message::ExtensionBrowser(b))
            .into()
    }))
    .spacing(4);

    let step = |n: &'a str, line: &'a str| {
        row![
            container(colored(n, |c| c.accent).size(12).font(BOLD))
                .center(24)
                .style(style::accent_soft(12.0)),
            text(line).size(14).width(Fill),
        ]
        .spacing(12)
        .align_y(Center)
    };
    let mut body = column![
        status,
        muted("What it does").size(13).font(SEMIBOLD),
        text("• Downloads you start in the browser go to BDownloader.").size(14),
        text("• Right-click a link and choose Download with BDownloader.").size(14),
        text("• Videos, YouTube too, get a Download this video bar.").size(14),
        muted("How to add it").size(13).font(SEMIBOLD),
        container(tabs).padding(4).style(style::panel),
    ]
    .spacing(12);
    // One click where the browser's store has the extension.
    if browser.one_click() {
        let hint = match browser {
            Browser::Firefox => "Firefox opens the add-on's page: click Add to Firefox.".to_owned(),
            Browser::Opera => "Opera opens the store page. The first time, it offers Install Chrome Extensions; then click Add to Opera.".to_owned(),
            _ => format!("{} opens the store page: click Add (or Get) there.", browser.name()),
        };
        body = body.push(
            row![
                button(text(format!("Add to {}", browser.name())).size(14))
                    .padding([10, 18])
                    .style(style::primary)
                    .on_press(Message::AddExtension(browser)),
                text(hint).size(14).width(Fill),
            ]
            .spacing(14)
            .align_y(Center),
        );
    }
    body = if browser == Browser::Firefox {
        // Firefox only keeps add-ons from its store, so there's no manual way.
        let page = crate::browser::FIREFOX_STORE_PAGE.to_owned();
        let page_copied = copied == Some(page.as_str());
        body.push(muted("Firefox didn't open? Open this page in Firefox:").size(13))
            .push(copy_field(page, page_copied))
    } else {
        let page = browser.extensions_page().to_owned();
        let folder =
            crate::browser::extension_dir().map_or("the extension folder next to BDownloader.exe".to_owned(), |dir| dir.display().to_string());
        let page_copied = copied == Some(page.as_str());
        let folder_copied = copied == Some(folder.as_str());
        if browser.one_click() {
            body = body.push(muted("Or add it yourself, without the store:").size(13));
        }
        body.push(step("1", "Copy this address and paste it into the browser's address bar:"))
            .push(copy_field(page, page_copied))
            .push(step("2", browser.developer_mode()))
            .push(step("3", "Click Load unpacked and choose this folder:"))
            .push(copy_field(folder, folder_copied))
    };
    body = body.push(muted("It shows as connected here once the browser has talked to BDownloader.").size(13));

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

/// A download's details: where it's saved, where it came from, and how it's going.
pub fn details<'a>(item: &'a Item, copied: Option<&str>, limit_text: &'a str) -> Element<'a, Message> {
    let status = match &item.state {
        State::Starting => "Starting".to_owned(),
        State::Downloading => "Downloading".to_owned(),
        State::Verifying => "Checking the file".to_owned(),
        State::Paused if item.queued => "Waiting in the queue".to_owned(),
        State::Paused => "Paused".to_owned(),
        State::Completed => "Completed".to_owned(),
        State::Failed(error) => format!("Failed: {error}"),
    };
    let progress = match item.size {
        Some(size) => format!("{} of {}", format::size(item.downloaded), format::size(size)),
        None => format::size(item.downloaded),
    };
    let connections = if item.state.is_running() && item.active_connections > 0 {
        format!("{} open (up to {})", item.active_connections, item.connections)
    } else {
        format!("Up to {}", item.connections)
    };
    let line = |label: &'a str, value: String| {
        row![muted(label).size(13).width(110), text(value).size(14).width(Fill)]
            .spacing(12)
            .align_y(Center)
    };
    let copyable = |value: String| {
        let is_copied = copied == Some(value.as_str());
        copy_field(value, is_copied)
    };

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
        line("Status", status),
        line("Size", progress),
        line("Connections", connections),
    ]
    .spacing(12);
    if let Some(source) = &item.youtube {
        let what = if source.video.is_some() { "Picture and sound" } else { "Sound only" };
        body = body.push(line("YouTube", what.to_owned()));
    }
    if let Some((kind, hash)) = &item.checksum {
        body = body.push(line("Checksum", format!("{kind:?} {hash}")));
    }
    if item.state != State::Completed {
        let id = item.id;
        let on = item.speed_limit_mb.is_some();
        let mut limit = row![
            muted("Speed").size(13).width(110),
            checkbox(on)
                .label("Limit this download to")
                .on_toggle(move |on| Message::ItemLimit(id, on))
                .size(18)
                .text_size(14)
                .spacing(10),
        ]
        .spacing(12)
        .align_y(Center);
        if on {
            limit = limit
                .push(
                    text_input("1", limit_text)
                        .on_input(move |typed| Message::ItemLimitText(id, typed))
                        .padding([6, 8])
                        .size(14)
                        .width(64)
                        .style(style::input),
                )
                .push(muted("MB/s").size(13));
        }
        body = body.push(limit);
    }
    body = body
        .push(muted("Saved in").size(13).font(SEMIBOLD))
        .push(copyable(item.dir.display().to_string()))
        .push(muted("Link").size(13).font(SEMIBOLD))
        .push(copyable(item.url.clone()));
    let page = item.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("referer"));
    if let Some((_, page)) = page {
        body = body.push(muted("From page").size(13).font(SEMIBOLD)).push(copyable(page.clone()));
    }

    let footer = row![
        space().width(Fill),
        button(text("Open folder").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::OpenDownloadFolder(item.id)),
        button(text("Close").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
    ]
    .spacing(10);

    frame("Download details", scrollable(body).style(style::scrollbar).into(), footer.into())
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
