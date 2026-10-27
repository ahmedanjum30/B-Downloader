//! The main window: sidebar, header and the download list.

use bdownloader_core::Category;
use iced::font::Weight;
use iced::widget::{
    Space, button, center, column, container, mouse_area, opaque, pick_list, progress_bar, row, scrollable, space, stack, text, text_input, tooltip,
};
use iced::{Center, Element, Fill, FillPortion, Font, Padding};

use crate::app::{App, Filter, Message, Modal, SettingsTab, Update, WhenDone};
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::style::{self, Pick};
use crate::{dialogs, update};
use crate::{format, store};

/// The sidebar's width.
const SIDEBAR_WIDTH: f32 = 236.0;
/// Room the status tabs and the speed and when-done controls need side by side.
const TOOLBAR_ONE_ROW: f32 = 840.0;

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
        Some(Modal::Settings(draft)) => modal(base, dialogs::settings(draft, app.update.status(), app.settings_tab)),
        Some(Modal::Report(dialog)) => modal(base, dialogs::report(dialog)),
        Some(Modal::Refresh(dialog)) => modal(base, dialogs::refresh(dialog)),
        Some(Modal::Remove { id, delete_file }) => match app.items.iter().find(|i| i.id == *id) {
            Some(item) => modal(base, dialogs::remove(item, *delete_file)),
            None => base.into(),
        },
        Some(Modal::Details(id)) => match app.items.iter().find(|i| i.id == *id) {
            Some(item) => modal(base, dialogs::details(item, app.copied.as_deref(), &app.item_limit_text)),
            None => base.into(),
        },
        Some(Modal::Extension(browser)) => modal(
            base,
            dialogs::extension(*browser, app.settings.extension_connected, app.copied.as_deref()),
        ),
        Some(Modal::PowerCountdown { action, deadline }) => {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            modal(base, dialogs::power_countdown(*action, left.as_secs_f64().ceil() as u64))
        }
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
    let logo = container(
        row![
            container(icon(&icons::DOWNLOAD, 16.0, |c| c.on_accent)).center(28).style(style::logo),
            text("BDownloader").size(16).font(SEMIBOLD),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([0, 8]);

    let filter_item = |filter: Filter| {
        let handle = match filter {
            Filter::All => &icons::INBOX,
            Filter::Active => &icons::ACTIVE,
            Filter::Completed => &icons::DONE,
            Filter::Paused => &icons::PAUSED,
            Filter::Failed => &icons::FAILED,
            Filter::Queue => &icons::CLOCK,
            Filter::Category(Category::Compressed) => &icons::COMPRESSED,
            Filter::Category(Category::Documents) => &icons::DOCUMENT,
            Filter::Category(Category::Music) => &icons::MUSIC,
            Filter::Category(Category::Programs) => &icons::PROGRAM,
            Filter::Category(Category::Video) => &icons::VIDEO,
        };
        nav_item(
            handle,
            filter.label(),
            Some(app.count(filter)),
            app.filter == filter,
            Message::Filter(filter),
        )
    };
    // Only the kinds of file there are downloads of (and the one shown).
    let kinds: Vec<Filter> = Category::ALL
        .map(Filter::Category)
        .into_iter()
        .filter(|&kind| app.filter == kind || app.count(kind) > 0)
        .collect();
    let mut filters = column![filter_item(Filter::All), filter_item(Filter::Queue)].spacing(2);
    if !kinds.is_empty() {
        filters = filters
            .push(container(muted("Types").size(12)).padding(Padding {
                top: 16.0,
                right: 10.0,
                bottom: 6.0,
                left: 10.0,
            }))
            .extend(kinds.into_iter().map(filter_item));
    }
    // Scrolls when the window is too short for every entry.
    let filters = scrollable(filters).height(Fill).spacing(4).style(style::scrollbar);

    let footer = column![
        container(space()).width(Fill).height(1).style(style::rule),
        space().height(8),
        nav_item(&icons::GLOBE, "Browser extension", None, false, Message::OpenExtension),
        nav_item(&icons::SETTINGS, "Settings", None, false, Message::OpenSettings),
        nav_item(&icons::HEART, "Support BDownloader", None, false, Message::OpenSupport),
    ]
    .spacing(2);

    container(column![logo, space().height(26), filters, space().height(12), footer])
        .padding(Padding {
            top: 20.0,
            right: 12.0,
            bottom: 14.0,
            left: 12.0,
        })
        .width(SIDEBAR_WIDTH)
        .height(Fill)
        .style(style::sidebar)
        .into()
}

fn nav_item<'a>(handle: &icons::Handle, label: &'a str, count: Option<usize>, selected: bool, message: Message) -> Element<'a, Message> {
    let pick: Pick = if selected { |c| c.text } else { |c| c.muted };
    let mut content = row![
        icon(handle, 16.0, pick),
        text(label).size(14).font(if selected { SEMIBOLD } else { FONT }).width(Fill)
    ]
    .spacing(10)
    .align_y(Center);
    if let Some(count) = count.filter(|&n| n > 0) {
        content = content.push(colored(count.to_string(), pick).size(12));
    }
    button(content)
        .padding([8, 10])
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
                top: 8.0,
                right: 12.0,
                bottom: 8.0,
                left: 36.0,
            })
            .size(14)
            .style(style::input),
        container(icon(&icons::SEARCH, 16.0, |c| c.muted)).padding(Padding {
            top: 10.0,
            right: 0.0,
            bottom: 0.0,
            left: 12.0,
        }),
    ];

    let title = column![text(app.filter.label()).size(20).font(SEMIBOLD), muted(subtitle).size(13)]
        .spacing(2)
        .width(Fill);
    // Room next to the sidebar.
    let room = app.width - SIDEBAR_WIDTH;
    let header = container(
        row![title, search.width(if room >= 760.0 { 240 } else { 180 }), add_button(room >= 680.0)]
            .spacing(8)
            .align_y(Center),
    )
    .padding([16, 24]);
    let divider = container(space()).width(Fill).height(1).style(style::rule);

    // Under the header: the status tabs and the controls for all downloads,
    // or the queue's own controls.
    let toolbar: Element<'_, Message> = if app.filter == Filter::Queue {
        queue_bar(app)
    } else {
        let controls = row![speed_limit(app), when_done(app)].spacing(8).align_y(Center);
        if app.items.is_empty() {
            row![space().width(Fill), controls].into()
        } else if room >= TOOLBAR_ONE_ROW {
            row![status_tabs(app), space().width(Fill), controls].align_y(Center).into()
        } else {
            column![status_tabs(app), controls].spacing(10).into()
        }
    };

    let items: Vec<&Item> = app.visible().collect();
    let list: Element<'_, Message> = if app.items.is_empty() {
        empty_state()
    } else if items.is_empty() {
        center(muted("Nothing here.").size(14)).into()
    } else {
        scrollable(column(items.into_iter().map(download_card)).spacing(8).padding(Padding {
            top: 0.0,
            right: 12.0,
            bottom: 14.0,
            left: 0.0,
        }))
        .height(Fill)
        .style(style::scrollbar)
        .into()
    };

    // Lined up with the cards, which leave room for the scrollbar.
    let toolbar = container(toolbar).padding(Padding {
        top: 0.0,
        right: 12.0,
        bottom: 0.0,
        left: 0.0,
    });
    let mut body = column![toolbar].spacing(14);
    if let Some(banner) = update_banner(app) {
        body = body.push(banner);
    }
    let body = body.push(list).padding(Padding {
        top: 14.0,
        right: 12.0,
        bottom: 0.0,
        left: 24.0,
    });
    column![header, divider, body].width(Fill).into()
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

/// The speed limit for all downloads together: a toggle and, while it's on,
/// the limit in MB/s. Changes apply at once, also to running downloads.
fn speed_limit(app: &App) -> Element<'_, Message> {
    let on = app.settings.limit_speed;
    let toggle = button(
        row![
            icon(&icons::GAUGE, 16.0, if on { |c| c.accent } else { |c| c.muted }),
            text(if on { "Limit to" } else { "No speed limit" }).size(14).font(SEMIBOLD),
        ]
        .spacing(8)
        .align_y(Center),
    )
    .padding([5, 8])
    .style(style::icon_button)
    .on_press(Message::LimitSpeed(!on));
    let mut control = row![toggle].spacing(6).align_y(Center);
    if on {
        control = control.push(
            text_input("5", &app.speed_limit_text)
                .on_input(Message::SpeedLimit)
                .padding([4, 8])
                .size(14)
                .width(56)
                .style(style::input),
        );
        control = control.push(container(muted("MB/s").size(13)).padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 0.0,
        }));
    }
    container(control).padding(2).style(style::outline).into()
}

/// All · Downloading · Completed · Paused · Failed, within what the sidebar shows.
fn status_tabs(app: &App) -> Element<'_, Message> {
    let tabs = row(Filter::STATUSES.map(|status| {
        let selected = app.status == status;
        let label = if status == Filter::All { "All" } else { status.label() };
        let mut content = row![text(label).size(14).font(SEMIBOLD)].spacing(6).align_y(Center);
        let count = app.count_status(status);
        if count > 0 {
            content = content.push(muted(count.to_string()).size(12));
        }
        button(content)
            .padding([5, 12])
            .style(style::tab(selected))
            .on_press(Message::Status(status))
            .into()
    }))
    .spacing(2);
    container(tabs).padding(4).style(style::panel).into()
}

/// Starts and stops the queue, and says how it runs.
fn queue_bar(app: &App) -> Element<'_, Message> {
    let settings = &app.settings;
    let waiting = app.items.iter().filter(|i| i.is_waiting_in_queue()).count();
    let mut line = if app.queue_running {
        format!("Running, {} at a time", settings.queue_at_once)
    } else {
        match waiting {
            0 => "Empty: add downloads with Download later".to_owned(),
            1 => "1 download waiting".to_owned(),
            n => format!("{n} downloads waiting"),
        }
    };
    if settings.queue_start_daily && store::parse_time(&settings.queue_start).is_some() {
        line += &format!(" · Starts every day at {}", settings.queue_start.trim());
    }
    if settings.queue_stop_daily && store::parse_time(&settings.queue_stop).is_some() {
        line += &format!(" · Stops at {}", settings.queue_stop.trim());
    }
    let control = if app.queue_running {
        button(
            row![icon(&icons::PAUSE, 15.0, |c| c.text), text("Stop queue").size(14).font(SEMIBOLD)]
                .spacing(8)
                .align_y(Center),
        )
        .padding([8, 12])
        .style(style::secondary)
        .on_press(Message::StopQueue)
    } else {
        button(
            row![icon(&icons::PLAY, 15.0, |c| c.on_accent), text("Start queue").size(14).font(SEMIBOLD)]
                .spacing(8)
                .align_y(Center),
        )
        .padding([8, 12])
        .style(style::primary)
        .on_press_maybe((waiting > 0).then_some(Message::StartQueue))
    };
    row![
        icon(&icons::CLOCK, 16.0, if app.queue_running { |c| c.accent } else { |c| c.muted }),
        muted(line).size(14).width(Fill),
        button(text("Schedule").size(14).font(SEMIBOLD))
            .padding([8, 12])
            .style(style::secondary)
            .on_press(Message::SettingsTab(SettingsTab::Queue)),
        control,
    ]
    .spacing(10)
    .align_y(Center)
    .into()
}

/// Sleep or shut down the PC once the downloads are done.
fn when_done(app: &App) -> Element<'_, Message> {
    let on = app.when_done != WhenDone::Nothing;
    let choice = pick_list(&WhenDone::ALL[..], Some(app.when_done), Message::WhenDone)
        .padding([5, 8])
        .text_size(14)
        .font(SEMIBOLD)
        .style(style::bare_pick_list);
    container(
        row![icon(&icons::POWER, 16.0, if on { |c| c.accent } else { |c| c.muted }), choice]
            .spacing(2)
            .align_y(Center),
    )
    .padding(Padding {
        top: 2.0,
        right: 2.0,
        bottom: 2.0,
        left: 10.0,
    })
    .style(style::outline)
    .into()
}

/// "+ Add download", or just "+" where space is short.
fn add_button<'a>(with_label: bool) -> Element<'a, Message> {
    let mut content = row![icon(&icons::PLUS, 16.0, |c| c.on_accent)].spacing(8).align_y(Center);
    if with_label {
        content = content.push(text("Add download").size(14).font(SEMIBOLD));
    }
    button(content)
        .padding(if with_label { [9, 14] } else { [9, 10] })
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
            add_button(true),
        ]
        .spacing(8)
        .align_x(Center),
    )
    .into()
}

/// The file's type, in a quiet square.
pub fn file_badge<'a>(extension: String) -> Element<'a, Message> {
    badge(extension, false)
}

fn badge<'a>(extension: String, failed: bool) -> Element<'a, Message> {
    container(text(extension).size(10).font(BOLD))
        .center(36)
        .style(style::file_badge(failed))
        .into()
}

/// What a download is doing: the word in its status pill and its colour,
/// then the details beside it and theirs.
fn status(item: &Item) -> (&'static str, Pick, String, Pick) {
    let sizes = || match (item.downloaded, item.size) {
        (0, Some(size)) => format::size(size),
        (0, None) => String::new(),
        (done, Some(size)) => format!("{} of {}", format::size(done), format::size(size)),
        (done, None) => format::size(done),
    };
    match &item.state {
        State::Paused | State::Failed(_) if item.awaiting_link => (
            "Waiting for link",
            |c| c.accent,
            "Click the download again in your browser".into(),
            |c| c.muted,
        ),
        State::Starting => ("Connecting", |c| c.accent, sizes(), |c| c.muted),
        // Only once data has stopped, so a brief reconnect while others keep going doesn't flash.
        State::Downloading if item.problem.is_some() && item.speed.rate() < 1024.0 => (
            "Reconnecting",
            |c| c.warning,
            capitalize(item.problem.as_deref().unwrap_or_default()),
            |c| c.muted,
        ),
        State::Downloading => ("Downloading", |c| c.accent, downloading_text(item), |c| c.muted),
        State::Verifying => ("Verifying", |c| c.accent, "Checking the checksum".into(), |c| c.muted),
        State::Paused if item.queued => ("Queued", |c| c.muted, sizes(), |c| c.muted),
        State::Paused => ("Paused", |c| c.warning, sizes(), |c| c.muted),
        State::Completed => (
            "Completed",
            |c| c.success,
            format!("{} · {}", item.size.map(format::size).unwrap_or_default(), item.host()),
            |c| c.muted,
        ),
        State::Failed(error) if item.link_may_have_expired() => (
            "Failed",
            |c| c.danger,
            format!("{} · the link may have expired", capitalize(error)),
            |c| c.danger,
        ),
        State::Failed(error) => ("Failed", |c| c.danger, capitalize(error), |c| c.danger),
    }
}

fn download_card(item: &Item) -> Element<'_, Message> {
    let id = item.id;
    let failed = matches!(item.state, State::Failed(_));
    let (word, word_pick, detail, detail_pick) = status(item);

    let name = text(format::middle_ellipsis(&item.display_name(), 70))
        .size(15)
        .font(SEMIBOLD)
        .wrapping(text::Wrapping::None);
    let pill = container(
        row![
            container(space()).width(6).height(6).style(style::dot(word_pick)),
            text(word).size(12).font(SEMIBOLD)
        ]
        .spacing(6)
        .align_y(Center),
    )
    .padding([1, 8])
    .style(style::pill);
    let meta = row![pill, colored(detail, detail_pick).size(13).wrapping(text::Wrapping::None)]
        .spacing(8)
        .align_y(Center);

    let mut actions = row![].spacing(2).align_y(Center);
    if item.state.is_running() {
        actions = actions.push(text(format!("{:.0}%", item.fraction() * 100.0)).size(14).font(SEMIBOLD));
        actions = actions.push(Space::new().width(6));
        actions = actions.push(action(&icons::PAUSE, "Pause", Message::Pause(id)));
    }
    // A fresh link only helps a download that already has part of a known-size file.
    let refreshable = item.size.is_some() && item.downloaded > 0 && item.file_name.is_some();
    match &item.state {
        State::Paused if item.queued => {
            actions = actions.push(text_action(None, "Start now", Message::Resume(id)));
        }
        State::Paused => {
            actions = actions.push(action(&icons::PLAY, "Resume", Message::Resume(id)));
            actions = actions.push(action(&icons::CLOCK, "Download later (add to queue)", Message::Enqueue(id)));
            if refreshable {
                actions = actions.push(action(&icons::LINK, "Refresh link", Message::OpenRefresh(id)));
            }
        }
        State::Failed(_) if refreshable => {
            actions = actions.push(text_action(Some(&icons::LINK), "Refresh link", Message::OpenRefresh(id)));
            actions = actions.push(action(&icons::RETRY, "Try again", Message::Resume(id)));
            actions = actions.push(action(&icons::CLOCK, "Try again later (add to queue)", Message::Enqueue(id)));
        }
        State::Failed(_) => {
            actions = actions.push(text_action(Some(&icons::RETRY), "Try again", Message::Resume(id)));
            actions = actions.push(action(&icons::CLOCK, "Try again later (add to queue)", Message::Enqueue(id)));
        }
        State::Completed => {
            actions = actions.push(action(&icons::OPEN, "Open", Message::OpenFile(id)));
            actions = actions.push(action(&icons::FOLDER, "Show in folder", Message::ShowInFolder(id)));
        }
        _ => {}
    }
    actions = actions.push(action(&icons::INFO, "Details", Message::ShowDetails(id)));
    let remove_tip = if item.state == State::Completed { "Remove" } else { "Cancel and remove" };
    actions = actions.push(action(&icons::TRASH, remove_tip, Message::Remove(id)));

    let top = row![badge(item.extension(), failed), column![name, meta].spacing(5).width(Fill), actions]
        .spacing(14)
        .align_y(Center);

    let mut body = column![top].spacing(12);
    let never_started = failed && item.downloaded == 0;
    if item.state != State::Completed && !never_started && !(item.queued && item.downloaded == 0) {
        body = body.push(progress(item));
    }
    container(body)
        .padding([14, 16])
        .width(Fill)
        .style(if failed { style::card_alert } else { style::card })
        .into()
}

/// An outline button with words: the next step a download is waiting for.
fn text_action<'a>(handle: Option<&icons::Handle>, label: &'a str, message: Message) -> Element<'a, Message> {
    let mut content = row![].spacing(6).align_y(Center);
    if let Some(handle) = handle {
        content = content.push(icon(handle, 15.0, |c| c.text));
    }
    button(content.push(text(label).size(13).font(SEMIBOLD)))
        .padding([6, 10])
        .style(style::secondary)
        .on_press(message)
        .into()
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
    if let Some(mb) = item.speed_limit_mb {
        parts.push(format!("limited to {} MB/s", format::number(mb)));
    }
    // Connections that are open but get no data are worth knowing about.
    match (item.active_connections, item.receiving) {
        (0, _) => {}
        (1, _) => parts.push("1 connection".to_owned()),
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
        State::Paused if item.queued => |c| c.muted,
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
