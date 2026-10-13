//! The main window: sidebar, header and the download list.

use iced::font::Weight;
use iced::widget::{
    Space, button, center, column, container, mouse_area, opaque, progress_bar, row, scrollable, space, stack, text, text_input, tooltip,
};
use iced::{Center, Color, Element, Fill, FillPortion, Font, Padding};

use crate::app::{App, Filter, Message, Modal, Update};
use crate::format;
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::style::{self, Colors, Pick};
use crate::{dialogs, update};

pub const FONT: Font = Font::with_name("Segoe UI");
pub const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..FONT
};
pub const BOLD: Font = Font {
    weight: Weight::Bold,
    ..FONT
};

pub fn view(app: &App) -> Element<'_, Message> {
    let base = container(row![sidebar(app), main_panel(app)])
        .width(Fill)
        .height(Fill)
        .style(style::app_background);
    match &app.modal {
        None => base.into(),
        Some(Modal::Add(dialog)) => modal(base, dialogs::add(dialog, app.existing_partial(dialog))),
        Some(Modal::Settings(draft)) => modal(base, dialogs::settings(draft, app.update.status())),
        Some(Modal::Refresh(dialog)) => modal(base, dialogs::refresh(dialog)),
        Some(Modal::Remove { id, delete_file }) => match app.items.iter().find(|i| i.id == *id) {
            Some(item) => modal(base, dialogs::remove(item, *delete_file)),
            None => base.into(),
        },
        Some(Modal::Extension) => modal(base, dialogs::extension(app.settings.extension_connected, app.copied.as_deref())),
    }
}

/// Shows `content` centered over a dimmed `base`. Clicking outside closes it.
fn modal<'a>(base: impl Into<Element<'a, Message>>, content: Element<'a, Message>) -> Element<'a, Message> {
    stack![
        base.into(),
        opaque(mouse_area(center(opaque(content)).style(style::backdrop)).on_press(Message::CloseModal))
    ]
    .into()
}

/// Text in one of the theme's colors.
pub fn colored<'a>(content: impl text::IntoFragment<'a>, pick: Pick) -> text::Text<'a> {
    text(content).style(move |theme| text::Style {
        color: Some(pick(style::colors(theme))),
    })
}

pub fn muted<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    colored(content, |c| c.muted)
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let logo = row![
        container(icon(&icons::DOWNLOAD, 18.0, |c| c.on_accent)).padding(8).style(style::logo),
        text("BDownloader").size(18).font(BOLD),
    ]
    .spacing(12)
    .align_y(Center);

    let filters = column(Filter::ALL.map(|filter| {
        let handle = match filter {
            Filter::All => &icons::INBOX,
            Filter::Active => &icons::ACTIVE,
            Filter::Completed => &icons::DONE,
            Filter::Paused => &icons::PAUSED,
            Filter::Failed => &icons::FAILED,
        };
        nav_item(
            handle,
            filter.label(),
            Some(app.count(filter)),
            app.filter == filter,
            Message::Filter(filter),
        )
    }))
    .spacing(4);

    let footer = column![
        nav_item(&icons::GLOBE, "Browser extension", None, false, Message::OpenExtension),
        nav_item(&icons::SETTINGS, "Settings", None, false, Message::OpenSettings),
    ]
    .spacing(4);

    container(column![logo, space().height(32), filters, space().height(Fill), footer])
        .padding(Padding {
            top: 24.0,
            right: 14.0,
            bottom: 18.0,
            left: 14.0,
        })
        .width(236)
        .height(Fill)
        .style(style::sidebar)
        .into()
}

fn nav_item<'a>(handle: &icons::Handle, label: &'a str, count: Option<usize>, selected: bool, message: Message) -> Element<'a, Message> {
    let pick: Pick = if selected { |c| c.accent } else { |c| c.muted };
    let mut content = row![
        icon(handle, 18.0, pick),
        text(label).size(14).font(if selected { SEMIBOLD } else { FONT }).width(Fill)
    ]
    .spacing(12)
    .align_y(Center);
    if let Some(count) = count.filter(|&n| n > 0) {
        content = content.push(colored(count.to_string(), pick).size(12).font(SEMIBOLD));
    }
    button(content)
        .padding([9, 12])
        .width(Fill)
        .style(style::nav(selected))
        .on_press(message)
        .into()
}

fn main_panel(app: &App) -> Element<'_, Message> {
    let running: Vec<&Item> = app.items.iter().filter(|i| i.state.is_running()).collect();
    let subtitle = if running.is_empty() {
        match app.items.len() {
            0 => "Paste a link to start".to_owned(),
            1 => "1 download".to_owned(),
            n => format!("{n} downloads"),
        }
    } else {
        let speed: f64 = running.iter().map(|i| i.speed.rate()).sum();
        format!("{} downloading · {}", running.len(), format::speed(speed))
    };

    let search = stack![
        text_input("Search downloads", &app.search)
            .on_input(Message::Search)
            .padding(Padding {
                top: 10.0,
                right: 12.0,
                bottom: 10.0,
                left: 38.0,
            })
            .size(14)
            .style(style::input),
        container(icon(&icons::SEARCH, 16.0, |c| c.muted)).padding(Padding {
            top: 12.0,
            right: 0.0,
            bottom: 0.0,
            left: 13.0,
        }),
    ]
    .width(260);

    let header = row![
        column![text(app.filter.label()).size(26).font(BOLD), muted(subtitle).size(13)]
            .spacing(2)
            .width(Fill),
        search,
        add_button(),
    ]
    .spacing(12)
    .align_y(Center);

    let items: Vec<&Item> = app.visible().collect();
    let list: Element<'_, Message> = if app.items.is_empty() {
        empty_state()
    } else if items.is_empty() {
        center(muted("Nothing here.").size(14)).into()
    } else {
        scrollable(column(items.into_iter().map(download_card)).spacing(10).padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 8.0,
            left: 0.0,
        }))
        .height(Fill)
        .into()
    };

    let mut content = column![header].spacing(24);
    if let Some(banner) = update_banner(app) {
        content = content.push(banner);
    }
    content
        .push(list)
        .spacing(24)
        .padding(Padding {
            top: 28.0,
            right: 18.0,
            bottom: 18.0,
            left: 32.0,
        })
        .width(Fill)
        .into()
}

/// Offers an update. Automatic updates stay silent unless the user started
/// one by hand or it failed then.
fn update_banner(app: &App) -> Option<Element<'_, Message>> {
    if app.update_dismissed {
        return None;
    }
    let small = |label: &'static str, message: Message, primary: bool| -> Element<'_, Message> {
        button(text(label).size(13).font(SEMIBOLD))
            .padding([7, 14])
            .style(if primary { style::primary } else { style::secondary })
            .on_press(message)
            .into()
    };
    let (line, actions): (String, Vec<Element<'_, Message>>) = match &app.update {
        Update::Available(release) => (
            format!("BDownloader {} is available.", release.version),
            vec![
                small("What's new", Message::OpenReleasePage, false),
                small(if update::is_installed() { "Update now" } else { "Download" }, Message::UpdateNow, true),
            ],
        ),
        Update::Downloading { .. } if app.update_manual => (app.update.status(), Vec::new()),
        Update::Ready { release, .. } if app.update_manual || !app.settings.auto_update => (
            format!("BDownloader {} is ready. Restart to finish updating.", release.version),
            vec![
                small("What's new", Message::OpenReleasePage, false),
                small("Restart now", Message::UpdateNow, true),
            ],
        ),
        Update::Failed(_) if app.update_manual => (app.update.status(), vec![small("Try again", Message::UpdateNow, false)]),
        _ => return None,
    };
    let mut bar = row![icon(&icons::DOWNLOAD, 18.0, |c| c.accent), text(line).size(14).font(SEMIBOLD).width(Fill)]
        .spacing(12)
        .align_y(Center);
    for action in actions {
        bar = bar.push(action);
    }
    bar = bar.push(
        button(icon(&icons::CLOSE, 15.0, |c| c.muted))
            .padding(6)
            .style(style::icon_button)
            .on_press(Message::DismissUpdate),
    );
    Some(container(bar).padding([10, 14]).width(Fill).style(style::accent_soft(12.0)).into())
}

fn add_button<'a>() -> Element<'a, Message> {
    button(
        row![icon(&icons::PLUS, 16.0, |c| c.on_accent), text("Add download").size(14).font(SEMIBOLD)]
            .spacing(8)
            .align_y(Center),
    )
    .padding([10, 18])
    .style(style::primary)
    .on_press(Message::OpenAdd)
    .into()
}

fn empty_state<'a>() -> Element<'a, Message> {
    center(
        column![
            container(icon(&icons::DOWNLOAD, 34.0, |c| c.accent))
                .center(84)
                .style(style::accent_soft(42.0)),
            space().height(8),
            text("No downloads yet").size(20).font(SEMIBOLD),
            muted("Copy a link and press Ctrl+V, or click Add download.").size(14),
            space().height(8),
            add_button(),
        ]
        .spacing(8)
        .align_x(Center),
    )
    .into()
}

/// Badge color by kind of file, the same in both themes.
pub fn kind_color(extension: &str) -> Color {
    let rgb = |r, g, b| Color::from_rgb8(r, g, b);
    match extension {
        "MP4" | "MKV" | "AVI" | "MOV" | "WEBM" | "WMV" | "FLV" | "M4V" => rgb(0xec, 0x48, 0x99),
        "MP3" | "FLAC" | "WAV" | "AAC" | "OGG" | "M4A" | "OPUS" => rgb(0xf5, 0x9e, 0x0b),
        "ZIP" | "RAR" | "7Z" | "TAR" | "GZ" | "XZ" | "BZ2" | "ZST" => rgb(0x8b, 0x5c, 0xf6),
        "PDF" | "DOC" | "DOCX" | "TXT" | "XLS" | "XLSX" | "PPT" | "PPTX" | "EPUB" | "CSV" => rgb(0x3b, 0x82, 0xf6),
        "EXE" | "MSI" | "APK" | "DMG" | "DEB" | "RPM" | "APPX" | "MSIX" => rgb(0x10, 0xb9, 0x81),
        "JPG" | "JPEG" | "PNG" | "GIF" | "WEBP" | "SVG" | "BMP" | "HEIC" => rgb(0x14, 0xb8, 0xa6),
        "ISO" | "IMG" | "BIN" | "VHD" | "VHDX" => rgb(0xef, 0x44, 0x44),
        _ => rgb(0x64, 0x74, 0x8b),
    }
}

pub fn file_badge<'a>(extension: String) -> Element<'a, Message> {
    let color = kind_color(&extension);
    container(text(extension).size(11).font(BOLD))
        .center(44)
        .style(style::tinted(color, 12.0))
        .into()
}

fn download_card(item: &Item) -> Element<'_, Message> {
    let id = item.id;
    let (status_pick, status): (Pick, String) = match &item.state {
        State::Paused | State::Failed(_) if item.awaiting_link => {
            (|c| c.accent, "Waiting for a new link: click the download again in your browser".into())
        }
        State::Starting => (|c| c.accent, "Connecting…".into()),
        // Only once data has stopped, so a brief reconnect while others keep going doesn't flash.
        State::Downloading if item.problem.is_some() && item.speed.rate() < 1024.0 => (
            |c| c.warning,
            format!("Reconnecting… {}", capitalize(item.problem.as_deref().unwrap_or_default())),
        ),
        State::Downloading => (|c| c.accent, downloading_text(item)),
        State::Verifying => (|c| c.accent, "Verifying checksum…".into()),
        State::Paused => (
            |c| c.warning,
            match (item.downloaded, item.size) {
                (0, _) => "Paused".into(),
                (done, Some(size)) => format!("Paused · {} of {}", format::size(done), format::size(size)),
                (done, None) => format!("Paused · {}", format::size(done)),
            },
        ),
        State::Completed => (
            |c| c.success,
            format!("{} · {}", item.size.map(format::size).unwrap_or_default(), item.host()),
        ),
        State::Failed(error) if item.link_may_have_expired() => (
            |c| c.danger,
            format!("{} · The link may have expired: use Refresh link", capitalize(error)),
        ),
        State::Failed(error) => (|c| c.danger, capitalize(error)),
    };

    let name = text(format::middle_ellipsis(&item.display_name(), 70))
        .size(15)
        .font(SEMIBOLD)
        .wrapping(text::Wrapping::None);
    let meta = colored(
        status,
        if item.state == State::Completed {
            |c: &Colors| c.muted
        } else {
            status_pick
        },
    )
    .size(13)
    .wrapping(text::Wrapping::None);

    let mut actions = row![].spacing(2).align_y(Center);
    if item.state.is_running() {
        actions = actions.push(colored(format!("{:.0}%", item.fraction() * 100.0), |c| c.accent).size(14).font(SEMIBOLD));
        actions = actions.push(Space::new().width(8));
        actions = actions.push(action(&icons::PAUSE, "Pause", Message::Pause(id)));
    }
    // A fresh link only helps a download that already has part of a known-size file.
    let refreshable = item.size.is_some() && item.downloaded > 0 && item.file_name.is_some();
    match &item.state {
        State::Paused => {
            actions = actions.push(action(&icons::PLAY, "Resume", Message::Resume(id)));
            if refreshable {
                actions = actions.push(action(&icons::LINK, "Refresh link", Message::OpenRefresh(id)));
            }
        }
        State::Failed(_) => {
            actions = actions.push(action(&icons::RETRY, "Try again", Message::Resume(id)));
            if refreshable {
                actions = actions.push(action(&icons::LINK, "Refresh link", Message::OpenRefresh(id)));
            }
        }
        State::Completed => {
            actions = actions.push(action(&icons::OPEN, "Open", Message::OpenFile(id)));
            actions = actions.push(action(&icons::FOLDER, "Show in folder", Message::ShowInFolder(id)));
        }
        _ => {}
    }
    let remove_tip = if item.state == State::Completed { "Remove" } else { "Cancel and remove" };
    actions = actions.push(action(&icons::TRASH, remove_tip, Message::Remove(id)));

    let top = row![file_badge(item.extension()), column![name, meta].spacing(4).width(Fill), actions]
        .spacing(14)
        .align_y(Center);

    let mut body = column![top].spacing(14);
    let never_started = matches!(item.state, State::Failed(_)) && item.downloaded == 0;
    if item.state != State::Completed && !never_started {
        body = body.push(progress(item));
    }
    container(body).padding([16, 18]).width(Fill).style(style::card).into()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn downloading_text(item: &Item) -> String {
    let mut parts = vec![match item.size {
        Some(size) => format!("{} of {}", format::size(item.downloaded), format::size(size)),
        None => format::size(item.downloaded),
    }];
    let rate = item.speed.rate();
    if rate > 0.0 {
        parts.push(format::speed(rate));
    }
    if let Some(eta) = item.eta() {
        parts.push(format::eta(eta));
    }
    // Connections that are open but get no data are worth knowing about.
    match (item.active_connections, item.receiving) {
        (0 | 1, _) => {}
        (open, receiving) if receiving < open => parts.push(format!("{open} connections · {receiving} receiving")),
        (open, _) => parts.push(format!("{open} connections")),
    }
    parts.join(" · ")
}

fn action<'a>(handle: &icons::Handle, tip: &'a str, message: Message) -> Element<'a, Message> {
    tooltip(
        button(icon(handle, 17.0, |c| c.muted))
            .padding(8)
            .style(style::icon_button)
            .on_press(message),
        container(text(tip).size(12)).padding([5, 9]).style(style::tooltip),
        tooltip::Position::Top,
    )
    .gap(4)
    .into()
}

/// The progress bar. While several connections are working, it shows each
/// part of the file separately, so you can see the connections fill it in.
fn progress(item: &Item) -> Element<'_, Message> {
    let pick: Pick = match item.state {
        State::Paused => |c| c.warning,
        State::Failed(_) => |c| c.danger,
        _ => |c| c.accent,
    };
    match item.size {
        Some(size) if size > 0 && item.segments.len() > 1 => segment_map(item, size),
        _ => progress_bar(0.0..=1.0, item.fraction()).girth(6).style(style::progress(pick)).into(),
    }
}

fn segment_map(item: &Item, size: u64) -> Element<'_, Message> {
    // (length, done) spans covering the whole file. Gaps between unfinished segments are done.
    let mut segments = item.segments.clone();
    segments.sort_by_key(|s| s.start);
    let mut spans: Vec<(u64, bool)> = Vec::new();
    let mut push = |len: u64, done: bool| match spans.last_mut() {
        Some((last, last_done)) if *last_done == done => *last += len,
        _ if len > 0 => spans.push((len, done)),
        _ => {}
    };
    let mut pos = 0;
    for s in &segments {
        push(s.start.saturating_sub(pos), true);
        push(s.received.saturating_sub(s.start), true);
        push(s.end.saturating_sub(s.received), false);
        pos = s.end;
    }
    push(size.saturating_sub(pos), true);

    row(spans.into_iter().map(|(len, done)| {
        let portion = ((len as f64 / size as f64) * 1000.0).round().max(1.0) as u16;
        let pick: Pick = if done { |c| c.accent } else { |c| c.track };
        container(space())
            .width(FillPortion(portion))
            .height(6)
            .style(style::segment(pick))
            .into()
    }))
    .spacing(2)
    .into()
}
