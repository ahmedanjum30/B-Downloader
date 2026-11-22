//! The dialogs shown over the main window.

use std::path::Path;

use bdownloader_core::{MAX_CONNECTIONS, TorrentInfo};
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, sensor, slider, space, text, text_editor, text_input};
use iced::{Center, Element, Fill, Length, Shrink, Theme};

use crate::app::{AddDialog, MAX_AT_ONCE, Message, Probe, RefreshDialog, ReportDialog, SettingsTab, WhenDone};
use crate::browser::Browser;
use crate::format;
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::store::{self, HashKind, Settings, ThemeChoice};
use crate::style;
use crate::themes::{self, PackState};
use crate::view::{bold, colored, file_badge, font, muted, semibold, txt};

fn frame<'a>(title: &'a str, body: Element<'a, Message>, footer: Element<'a, Message>) -> Element<'a, Message> {
    sized_frame(title, body, footer, 540.0)
}

thread_local! {
    /// Dialogs being built as a window's whole contents (see `bare`).
    static BARE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Builds dialogs as a window of their own: no card around them (the window
/// is the dialog) and no close button of their own (the title bar has one).
pub fn bare<T>(build: impl FnOnce() -> T) -> T {
    BARE.set(true);
    let built = build();
    BARE.set(false);
    built
}

fn sized_frame<'a>(title: &'a str, body: Element<'a, Message>, footer: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    let mut header = row![txt(title).size(20).font(bold()).width(Fill)].align_y(Center);
    if BARE.get() {
        return container(column![header, body, footer].spacing(22))
            .padding(26)
            .width(width)
            .style(style::dialog_window)
            .into();
    }
    header = header.push(
        button(icon(&icons::CLOSE, 18.0, |c| c.muted))
            .padding(6)
            .style(style::icon_button)
            .on_press(Message::CloseModal),
    );
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
    column![muted(label).size(13).font(semibold()), content.into()].spacing(8).into()
}

fn input<'a>(placeholder: &'a str, value: &'a str) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value).font(font()).padding(12).size(14).style(style::input)
}

fn folder_picker<'a>(dir: &Path) -> Element<'a, Message> {
    row![
        container(
            txt(format::middle_ellipsis(&dir.display().to_string(), 52))
                .size(14)
                .wrapping(text::Wrapping::None)
        )
        .padding(12)
        .width(Fill)
        .clip(true)
        .style(style::panel),
        button(txt("Browse").size(14))
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
        container(txt(value.to_string()).size(15).font(semibold())).center_x(44),
        step(&icons::PLUS, value < max, value + 1),
        slider(1..=max as u32, value as u32, move |n| on_change(n as usize))
            .width(Fill)
            .style(style::slider),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn footer<'a>(confirm: &'a str, on_confirm: Option<Message>) -> Element<'a, Message> {
    row![
        space().width(Fill),
        cancel(),
        button(txt(confirm).size(14).font(semibold()))
            .padding([9, 16])
            .style(style::primary)
            .on_press_maybe(on_confirm),
    ]
    .spacing(8)
    .into()
}

fn cancel<'a>() -> Element<'a, Message> {
    button(txt("Cancel").size(14).font(semibold()))
        .padding([9, 14])
        .style(style::ghost)
        .on_press(Message::CloseModal)
        .into()
}

/// `existing`: an unfinished download of the same file (its id and how much
/// of it is done), which this link can continue instead of starting over.
pub fn add(dialog: &AddDialog, existing: Option<(u64, u64)>) -> Element<'_, Message> {
    let url = input("https://example.com/file.zip or a magnet link", &dialog.url)
        .on_input(Message::AddUrl)
        .on_paste(Message::AddUrlPasted)
        .on_submit(Message::AddUrlSubmit);
    let torrent_file = button(
        row![icon(&icons::FOLDER, 16.0, |c| c.text), txt("Torrent file").size(14)]
            .spacing(8)
            .align_y(Center),
    )
    .padding([10, 14])
    .style(style::secondary)
    .on_press(Message::AddTorrentFile);

    let mut body = column![field("Link", row![url, torrent_file].spacing(8).align_y(Center))].spacing(18);

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
            Probe::Checking if dialog.is_torrent() && dialog.url.trim().to_ascii_lowercase().starts_with("magnet:") => Some(
                muted("Getting the torrent's file list from peers… This can take a minute.")
                    .size(13)
                    .into(),
            ),
            Probe::Checking if dialog.is_torrent() => Some(muted("Reading the torrent…").size(13).into()),
            Probe::Checking => Some(muted("Checking link…").size(13).into()),
            Probe::Failed(_) if dialog.certificate_failed() => Some(certificate_warning()),
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
    // A torrent: what's in it, to choose from.
    if let Some(torrent) = &dialog.torrent {
        let count = dialog.selected.iter().filter(|on| **on).count();
        let size = if count == torrent.files.len() {
            format!("{} · {}", format::size(dialog.selected_size()), plural(torrent.files.len(), "file"))
        } else {
            format!("{} · {count} of {} files", format::size(dialog.selected_size()), torrent.files.len())
        };
        let badge = if torrent.multi_file {
            "BT".to_owned()
        } else {
            extension_of(&torrent.name)
        };
        let about = format!("{size} · from other people (peers) · can resume");
        body = body.push(summary(badge, format::middle_ellipsis(&torrent.name, 56), about, |c| c.success));
        if torrent.files.len() > 1 {
            body = body.push(torrent_files(dialog));
        }
        body = body.push(field("Save to", folder_picker(&dialog.save_dir())));
        let footer = add_footer(dialog);
        return frame("Add torrent", body.into(), footer);
    }
    if let Some(probe) = probe {
        body = body.push(probe);
    }
    if let Some((id, done)) = existing {
        body = body.push(
            container(
                row![
                    icon(&icons::LINK, 18.0, |c| c.accent),
                    txt(format!("You already have {} of this file in an unfinished download.", format::size(done)))
                        .size(14)
                        .width(Fill),
                    button(txt("Continue that download").size(13).font(semibold()))
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
        .font(font())
        .spacing(10);
    let mut checksum = column![row![icon(&icons::SHIELD, 18.0, |c| c.muted), verify].spacing(10).align_y(Center)].spacing(10);
    if dialog.verify {
        checksum = checksum.push(
            row![
                pick_list(&HashKind::ALL[..], Some(dialog.hash_kind), Message::AddHashKind)
                    .font(font())
                    .padding(11)
                    .text_size(14)
                    .width(120),
                input("Expected checksum (hex)", &dialog.hash).on_input(Message::AddHash),
            ]
            .spacing(8),
        );
    }
    body = body.push(checksum);

    frame("Add download", body.into(), add_footer(dialog))
}

fn add_footer(dialog: &AddDialog) -> Element<'_, Message> {
    row![
        space().width(Fill),
        cancel(),
        button(txt("Download later").size(14).font(semibold()))
            .padding([9, 14])
            .style(style::secondary)
            .on_press_maybe(dialog.can_submit().then_some(Message::ConfirmAddLater)),
        button(txt("Download").size(14).font(semibold()))
            .padding([10, 20])
            .style(style::primary)
            .on_press_maybe(dialog.can_submit().then_some(Message::ConfirmAdd)),
    ]
    .spacing(10)
    .into()
}

/// The torrent's files, each with a checkbox, and one for all of them.
fn torrent_files(dialog: &AddDialog) -> Element<'_, Message> {
    let Some(torrent) = &dialog.torrent else { return space().into() };
    file_list(torrent, dialog.selected.clone(), Message::AddTorrentSelectAll, Message::AddTorrentSelect)
}

/// Whether one of a torrent download's files is chosen.
fn torrent_file_chosen(item: &Item, index: usize) -> bool {
    item.torrent
        .as_ref()
        .and_then(|t| t.files.as_ref())
        .is_none_or(|files| files.contains(&index))
}

fn torrent_hash(item: &Item) -> &str {
    item.torrent.as_ref().map_or("", |t| t.info_hash.as_str())
}

/// A torrent's files with a checkbox each (`selected`, in the same order), and one for all.
fn file_list<'a>(
    torrent: &'a TorrentInfo,
    selected: Vec<bool>,
    on_all: impl Fn(bool) -> Message + 'a,
    on_file: impl Fn(usize, bool) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let all = selected.iter().all(|on| *on);
    let chosen: u64 = torrent.files.iter().zip(&selected).filter(|(_, on)| **on).map(|(f, _)| f.size).sum();
    let header = row![
        checkbox(all)
            .label("All files")
            .on_toggle(on_all)
            .size(18)
            .text_size(14)
            .font(font())
            .spacing(10)
            .width(Fill),
        muted(format!(
            "{} of {}",
            format::size(chosen),
            format::size(torrent.files.iter().map(|f| f.size).sum())
        ))
        .size(13),
    ]
    .align_y(Center);
    let files = column(torrent.files.iter().zip(selected).enumerate().map(|(i, (file, on))| {
        let on_file = on_file.clone();
        row![
            checkbox(on)
                .label(format::middle_ellipsis(&file.path, 60))
                .on_toggle(move |on| on_file(i, on))
                .size(16)
                .text_size(13)
                .font(font())
                .spacing(10)
                .width(Fill),
            muted(format::size(file.size)).size(12),
        ]
        .spacing(12)
        .align_y(Center)
        .into()
    }))
    .spacing(8)
    .padding(iced::Padding {
        right: 12.0,
        ..iced::Padding::ZERO
    });
    container(
        column![
            header,
            container(space()).width(Fill).height(1).style(style::rule),
            scrollable(files).height(Shrink).style(style::scrollbar),
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Fill)
    .max_height(220)
    .style(style::panel)
    .into()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// The extension a file-type badge shows for a name, such as "ZIP".
fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()))
        .map_or("FILE".to_owned(), |(_, ext)| ext.to_ascii_uppercase())
}

/// The link's certificate failed: why, and the way past it for a network the user trusts.
fn certificate_warning<'a>() -> Element<'a, Message> {
    column![
        colored("The site's security certificate isn't trusted by Windows.", |c| c.danger)
            .size(13)
            .font(semibold()),
        muted(
            "Often a work or school network inspecting HTTPS. It can also be someone changing the file on the way: \
             only download anyway on a network you trust."
        )
        .size(13),
        button(txt("Download anyway (skip certificate check)").size(13).font(semibold()))
            .padding([7, 12])
            .style(style::secondary)
            .on_press(Message::AddSkipCertificateCheck),
    ]
    .spacing(6)
    .into()
}

/// A line explaining the setting above it.
fn hint<'a>(content: &'a str) -> Element<'a, Message> {
    muted(content).size(12).into()
}

fn choices<'a, T: Copy + PartialEq>(options: &[(T, &'a str)], current: T, on_press: fn(T) -> Message) -> Element<'a, Message> {
    let buttons = row(options.iter().map(|&(value, label)| {
        button(txt(label).size(14).width(Fill).center())
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
            button(txt("Check now").size(13))
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
                     versions and B-Downloader's log, for you to check before sending."
                ),
                row![
                    button(txt("Report a problem").size(14).font(semibold()))
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
                    button(txt("Open log folder").size(14))
                        .padding([9, 16])
                        .style(style::secondary)
                        .on_press(Message::OpenLogs),
                ],
            ]
            .spacing(10)
        ),
        field(
            "Questions and ideas",
            hint("Ask in Discussions on GitHub: github.com/ahmedanjum30/B-Downloader/discussions"),
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
            checkbox(enabled)
                .label(label)
                .on_toggle(on_toggle)
                .size(18)
                .text_size(14)
                .font(font())
                .spacing(10),
            text_input("HH:MM", value)
                .font(font())
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
                hint("24-hour time. B-Downloader has to be running then; it can be in the tray."),
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
        .font(font())
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
            "When I close the window",
            column![
                choices(
                    &[(true, "Keep running in the tray"), (false, "Exit B-Downloader")],
                    draft.close_to_tray,
                    Message::SettingsCloseToTray
                ),
                hint("In the tray, B-Downloader still takes downloads from the browser and runs the queue's schedule."),
            ]
            .spacing(8)
        ),
    ]
    .spacing(22)
    .into()
}

fn themes_section<'a>(draft: &Settings, packs: &'a [PackState; 2]) -> Element<'a, Message> {
    // The downloaded ones join the built-in choices once they're added.
    let mut options = vec![
        (ThemeChoice::System, "System"),
        (ThemeChoice::Dark, "Dark"),
        (ThemeChoice::Light, "Light"),
    ];
    for (pack, state) in themes::PACKS.iter().zip(packs) {
        if let (PackState::Ready, Some(choice)) = (state, crate::app::choice_of(pack.look)) {
            options.push((choice, pack.name));
        }
    }
    let list = column(themes::PACKS.iter().zip(packs).map(|(pack, state)| theme_pack(pack, state))).spacing(8);
    column![
        field("Theme", choices(&options, draft.theme, Message::SettingsTheme)),
        field(
            "Hand-drawn themes",
            column![
                list,
                hint("Downloaded when you add them, so B-Downloader itself stays small. Their fonts are free (SIL Open Font License)."),
            ]
            .spacing(10)
        ),
    ]
    .spacing(22)
    .into()
}

/// A downloadable theme: a sample in its own colors and handwriting, and
/// what can be done with it.
fn theme_pack<'a>(pack: &'a themes::Pack, state: &'a PackState) -> Element<'a, Message> {
    let colors = pack.look.colors();
    let sample = container(text("Aa").size(18).font(colors.font))
        .center_x(52)
        .center_y(40)
        .style(style::swatch(pack.look));
    let mut about = column![txt(pack.name).size(14).font(semibold()), muted(pack.about).size(12)].spacing(2);
    if let PackState::Failed(reason) = state {
        about = about.push(colored(format!("Couldn't download it: {reason}"), |c| c.danger).size(12));
    }
    let action: Element<'a, Message> = match state {
        PackState::Missing => button(txt(format!("Add · {}", format::size(pack.size))).size(13))
            .padding([6, 12])
            .style(style::secondary)
            .on_press(Message::ThemeAdd(pack.look))
            .into(),
        PackState::Failed(_) => button(txt("Try again").size(13))
            .padding([6, 12])
            .style(style::secondary)
            .on_press(Message::ThemeAdd(pack.look))
            .into(),
        PackState::Downloading => button(txt("Downloading…").size(13)).padding([6, 12]).style(style::secondary).into(),
        PackState::Ready => button(txt("Remove").size(13))
            .padding([6, 12])
            .style(style::ghost)
            .on_press(Message::ThemeRemove(pack.look))
            .into(),
    };
    container(row![sample, about.width(Fill), action].spacing(14).align_y(Center))
        .padding(10)
        .width(Fill)
        .style(style::outline)
        .into()
}

fn torrents<'a>(draft: &'a Settings, seed_ratio_text: &'a str) -> Element<'a, Message> {
    let mut seeding = column![
        checkbox(draft.seed)
            .label("Keep sharing finished torrents")
            .on_toggle(Message::SettingsSeed)
            .size(18)
            .text_size(14)
            .font(font())
            .spacing(10),
    ]
    .spacing(8);
    if draft.seed {
        seeding = seeding.push(
            row![
                muted("Until they've uploaded").size(13),
                text_input("1", seed_ratio_text)
                    .font(font())
                    .on_input(Message::SettingsSeedRatio)
                    .padding([6, 8])
                    .size(14)
                    .width(64)
                    .style(style::input),
                muted("times their size").size(13),
            ]
            .spacing(8)
            .align_y(Center),
        );
    }
    seeding = seeding.push(hint(
        "Torrents download from other people, and work because people share back. \
         Sharing as much as you downloaded (1) is the usual courtesy.",
    ));
    column![
        field("Sharing (seeding)", seeding),
        field(
            "Opening torrents",
            hint(
                "Magnet links and .torrent files open in B-Downloader from the browser extension, the Add download \
                 dialog, or Windows: choose B-Downloader for them in Settings > Apps > Default apps.",
            ),
        ),
        field(
            "Choosing files",
            hint("A torrent's details (the i button) list its files. Tick or untick them at any time, also while it downloads."),
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
            hint(
                "For all downloads together, torrents too: the gauge button in the top bar. \
                 For one download: its details (the i button)."
            ),
        ),
    ]
    .spacing(22)
    .into()
}

/// `motion`: how far the section is in (0 to 1), as it fades in after its
/// tab is picked, and the height it's shown at while it grows or shrinks.
pub fn settings<'a>(
    draft: &'a Settings,
    update_status: String,
    tab: SettingsTab,
    seed_ratio_text: &'a str,
    (tab_shown, section_height): (f32, Option<f32>),
    theme: &Theme,
    packs: &'a [PackState; 2],
) -> Element<'a, Message> {
    let tabs = column(SettingsTab::ALL.map(|t| {
        button(txt(t.label()).size(14).font(if t == tab { semibold() } else { crate::view::font() }))
            .padding([8, 10])
            .width(Fill)
            .style(style::nav(t == tab))
            .on_press(Message::SettingsTab(t))
            .into()
    }))
    .spacing(2)
    .width(140);
    let (content, about) = match tab {
        SettingsTab::General => (general(draft), "Where downloads go, and what closing the window does."),
        SettingsTab::Themes => (themes_section(draft, packs), "How B-Downloader looks."),
        SettingsTab::Downloads => (downloads(draft), "How each download uses your connection."),
        SettingsTab::Torrents => (
            torrents(draft, seed_ratio_text),
            "Magnet links and .torrent files, and sharing them back.",
        ),
        SettingsTab::Queue => (queue_settings(draft), "Downloads saved for later, and when they run."),
        SettingsTab::Updates => (updates(draft, update_status), "How B-Downloader stays up to date."),
        SettingsTab::Help => (help(), "Report a problem, or find the log."),
    };
    let section = column![
        column![txt(tab.label()).size(18).font(semibold()), muted(about).size(13)].spacing(2),
        container(space()).width(Fill).height(1).style(style::rule),
        content,
    ]
    .spacing(18);
    // Measured at its natural height (the scrollable lets it be taller than
    // what shows), so the dialog can grow or shrink to it smoothly instead
    // of jumping when another tab is picked.
    let section = sensor(crate::view::faded(theme, tab_shown, section.into()))
        .on_show(|size| Message::SettingsSection(size.height))
        .on_resize(|size| Message::SettingsSection(size.height));
    let section = scrollable(section)
        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::hidden()))
        .height(section_height.map_or(Shrink, Length::Fixed));
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
                    .font(font())
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
            .label("Include B-Downloader's log (the latest part)")
            .on_toggle(Message::ReportIncludeLog)
            .size(18)
            .text_size(14)
            .font(font())
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
            txt(format!("Downloads are done. Your PC will {verb} in {seconds} seconds."))
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
            colored("Copied", |c| c.success).size(13).font(semibold())
        ]
        .spacing(6)
        .align_y(Center)
        .into()
    } else {
        row![icon(&icons::COPY, 15.0, |c| c.muted), muted("Copy").size(13).font(semibold())]
            .spacing(6)
            .align_y(Center)
            .into()
    };
    // The whole field is the button, so a click anywhere on it copies. Long
    // values (links with tokens) are shortened in the middle to fit on one
    // line, and clipped if wide letters still overrun; the copy is whole.
    button(
        row![
            container(
                txt(format::middle_ellipsis(&value, 50))
                    .size(14)
                    .font(semibold())
                    .wrapping(text::Wrapping::None)
            )
            .width(Fill)
            .clip(true),
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
        ("Connected", "Downloads from your browser come to B-Downloader.", |c| c.success)
    } else {
        ("Not connected", "Add the extension to your browser.", |c| c.muted)
    };
    let status = container(
        row![
            container(icon(&icons::GLOBE, 22.0, |c| c.accent))
                .center(44)
                .style(style::accent_soft(12.0)),
            column![colored(title, pick).size(15).font(semibold()), muted(detail).size(13)].spacing(3),
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(14)
    .width(Fill)
    .style(style::panel);

    // One tab per browser; the steps below are for the chosen one.
    let tabs = row(Browser::tabs().iter().map(|&b| {
        button(txt(b.name()).size(13).width(Fill).center())
            .padding([7, 4])
            .width(Fill)
            .style(style::tab(b == browser))
            .on_press(Message::ExtensionBrowser(b))
            .into()
    }))
    .spacing(4);

    let step = |n: &'a str, line: &'a str| {
        row![
            container(colored(n, |c| c.accent).size(12).font(bold()))
                .center(24)
                .style(style::accent_soft(12.0)),
            txt(line).size(14).width(Fill),
        ]
        .spacing(12)
        .align_y(Center)
    };
    // Safari: the extension comes inside the app; Safari only has to allow it.
    // It isn't signed by Apple yet, so Safari takes it only with "Allow
    // unsigned extensions", which it turns off whenever it quits.
    if browser == Browser::Safari {
        let body = column![
            status,
            muted("How to add it").size(13).font(semibold()),
            container(tabs).padding(4).style(style::panel),
            step("1", "In Safari, open Settings, then Advanced, and turn on Show features for web developers."),
            step("2", "Open the Developer tab and turn on Allow unsigned extensions. Safari asks for your Mac's password."),
            step("3", "Open the Extensions tab and turn on B-Downloader. Allow it on all websites."),
            muted("Safari turns Allow unsigned extensions off when it quits, until B-Downloader is signed by Apple: after restarting Safari, repeat step 2.").size(13),
            muted("In Safari, downloads don't come to B-Downloader by themselves: right-click a link and choose Download with B-Downloader, or use the Download this video bar.").size(13),
        ]
        .spacing(12);
        let footer = row![
            space().width(Fill),
            button(txt("Close").size(14))
                .padding([10, 18])
                .style(style::secondary)
                .on_press(Message::CloseModal),
        ];
        return frame("Browser extension", body.into(), footer.into());
    }

    let mut body = column![
        status,
        muted("What it does").size(13).font(semibold()),
        txt("• Downloads you start in the browser go to B-Downloader.").size(14),
        txt("• Right-click a link and choose Download with B-Downloader.").size(14),
        txt("• Videos, YouTube too, get a Download this video bar.").size(14),
        muted("How to add it").size(13).font(semibold()),
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
                button(txt(format!("Add to {}", browser.name())).size(14))
                    .padding([10, 18])
                    .style(style::primary)
                    .on_press(Message::AddExtension(browser)),
                txt(hint).size(14).width(Fill),
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
        let body = body
            .push(step("1", "Copy this address and paste it into the browser's address bar:"))
            .push(copy_field(page, page_copied))
            .push(step("2", browser.developer_mode()));
        if cfg!(target_os = "macos") {
            body.push(step(
                "3",
                "Click Load unpacked, then choose Users, your name, B-Downloader, extension, and click Select. This folder:",
            ))
            .push(copy_field(folder, folder_copied))
        } else {
            body.push(step("3", "Click Load unpacked and choose this folder:"))
                .push(copy_field(folder, folder_copied))
        }
    };
    body = body.push(muted("It shows as connected here once the browser has talked to B-Downloader.").size(13));

    let footer = row![
        space().width(Fill),
        button(txt("Close").size(14))
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
                txt(format::middle_ellipsis(&dialog.name, 48)).size(15).font(semibold()),
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

    let mut body = column![
        file,
        txt(
            "Download links often stop working after a while. Give B-Downloader a new link to the same file, and it continues from where it stopped."
        )
        .size(14),
    ]
    .spacing(16);

    if dialog.referrer.is_some() {
        body = body.push(
            row![
                button(txt("Open download page").size(14))
                    .padding([10, 16])
                    .style(style::secondary)
                    .on_press(Message::RefreshFromPage),
                muted("Then click the download again in your browser. B-Downloader picks up the new link.").size(13),
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
                txt(format::middle_ellipsis(&item.display_name(), 48)).size(15).font(semibold()),
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
        body = body.push(txt("It's removed from the list. The file itself stays where it is, unless you tick the box.").size(14));
        body = body.push(
            checkbox(delete_file)
                .label("Also delete the file from disk (moves it to the Recycle Bin)")
                .on_toggle(Message::RemoveDeleteFile)
                .size(18)
                .text_size(14)
                .font(font())
                .spacing(10),
        );
    } else {
        let done = if item.downloaded > 0 {
            format!("The {} downloaded so far will be deleted.", format::size(item.downloaded))
        } else {
            "Nothing has been downloaded yet.".to_owned()
        };
        body = body.push(txt(format!("The download stops and is removed from the list. {done}")).size(14));
    }

    let footer = row![
        space().width(Fill),
        button(txt("Cancel").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
        button(
            txt(if finished && !delete_file { "Remove" } else { "Remove and delete" })
                .size(14)
                .font(semibold())
        )
        .padding([10, 20])
        .style(style::danger)
        .on_press(Message::ConfirmRemove),
    ]
    .spacing(10);

    frame("Remove download", body.into(), footer.into())
}

/// A download's details: where it's saved, where it came from, and how it's going.
pub fn details<'a>(item: &'a Item, copied: Option<&str>, limit_text: &'a str, files_of: Option<&'a TorrentInfo>) -> Element<'a, Message> {
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
    let (connections_label, connections) = if item.is_torrent() {
        let connected = match (&item.seeding, item.state.is_running()) {
            (Some(seeding), _) => Some(seeding.peers),
            (None, true) => Some(item.active_connections),
            (None, false) => None,
        };
        let peers = match (item.swarm, connected) {
            (Some(swarm), Some(n)) => format!("{} seeds · {} peers · connected to {n}", swarm.seeds, swarm.peers),
            (Some(swarm), None) => format!("{} seeds · {} peers", swarm.seeds, swarm.peers),
            (None, Some(n)) => format!("Connected to {}", plural(n, "peer")),
            (None, None) => "None while stopped".to_owned(),
        };
        ("Swarm", peers)
    } else if item.state.is_running() && item.active_connections > 0 {
        ("Connections", format!("{} open (up to {})", item.active_connections, item.connections))
    } else {
        ("Connections", format!("Up to {}", item.connections))
    };
    let line = |label: &'a str, value: String| {
        row![muted(label).size(13).width(110), txt(value).size(14).width(Fill)]
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
                txt(format::middle_ellipsis(&item.display_name(), 48)).size(15).font(semibold()),
            ]
            .spacing(14)
            .align_y(Center),
        )
        .padding(12)
        .width(Fill)
        .style(style::panel),
        line("Status", status),
        line("Size", progress),
        line(connections_label, connections),
    ]
    .spacing(12);
    if let Some(torrent) = &item.torrent {
        let shared = format!("{} uploaded · ratio {:.2}", format::size(torrent.uploaded), item.ratio().unwrap_or(0.0));
        let sharing = match (&item.seeding, torrent.seeded) {
            (Some(seeding), _) => format!("{shared} · sharing at {}", format::speed(seeding.upload_rate)),
            (None, _) if item.state.is_running() && item.upload_rate > 0.0 => {
                format!("{shared} · sharing at {}", format::speed(item.upload_rate))
            }
            (None, true) if item.state == State::Completed => format!("{shared} · done sharing"),
            _ => shared,
        };
        body = body.push(line("Shared", sharing));
        if let Some(info) = files_of.filter(|t| t.info_hash == torrent_hash(item) && t.files.len() > 1) {
            let chosen = |index: usize| torrent_file_chosen(item, index);
            body = body.push(muted("Files").size(13).font(semibold()));
            body = body.push(file_list(
                info,
                info.files.iter().map(|f| chosen(f.index)).collect(),
                move |on| Message::ItemTorrentAllFiles(item.id, on),
                move |i, on| Message::ItemTorrentFile(item.id, i, on),
            ));
        }
    }
    if let Some(source) = &item.youtube {
        let what = if source.video.is_some() { "Picture and sound" } else { "Sound only" };
        body = body.push(line("YouTube", what.to_owned()));
    }
    if let Some((kind, hash)) = &item.checksum {
        body = body.push(line("Checksum", format!("{kind:?} {hash}")));
    }
    if item.skip_certificate_check {
        body = body.push(line("Security", "Certificate check off for this download, as you chose".to_owned()));
    }
    // Torrents follow the limit for all downloads only.
    if item.state != State::Completed && !item.is_torrent() {
        let id = item.id;
        let on = item.speed_limit_mb.is_some();
        let mut limit = row![
            muted("Speed").size(13).width(110),
            checkbox(on)
                .label("Limit this download to")
                .on_toggle(move |on| Message::ItemLimit(id, on))
                .size(18)
                .text_size(14)
                .font(font())
                .spacing(10),
        ]
        .spacing(12)
        .align_y(Center);
        if on {
            limit = limit
                .push(
                    text_input("1", limit_text)
                        .font(font())
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
        .push(muted("Saved in").size(13).font(semibold()))
        .push(copyable(item.dir.display().to_string()))
        .push(muted(if item.is_torrent() { "Torrent" } else { "Link" }).size(13).font(semibold()))
        .push(copyable(item.url.clone()));
    let page = item.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("referer"));
    if let Some((_, page)) = page {
        body = body.push(muted("From page").size(13).font(semibold())).push(copyable(page.clone()));
    }

    let footer = row![
        space().width(Fill),
        button(txt("Open folder").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::OpenDownloadFolder(item.id)),
        button(txt("Close").size(14))
            .padding([10, 18])
            .style(style::secondary)
            .on_press(Message::CloseModal),
    ]
    .spacing(10);

    frame("Download details", scrollable(body).style(style::scrollbar).into(), footer.into())
}

/// What's about to be downloaded: a file-type badge, the size and how it downloads.
fn summary<'a>(badge: String, size: String, support: impl text::IntoFragment<'a>, pick: style::Pick) -> Element<'a, Message> {
    container(
        row![
            file_badge(badge),
            column![txt(size).size(15).font(semibold()), colored(support, pick).size(13)].spacing(3)
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(12)
    .width(Fill)
    .style(style::panel)
    .into()
}
