//! The main window: sidebar, header and the download list.

use bdownloader_core::Category;
use iced::font::Weight;
use iced::widget::{
    Space, button, center, column, container, mouse_area, opaque, pick_list, progress_bar, row, scrollable, space, stack, text, text_input, themer,
    tooltip,
};
use iced::{Center, Element, Fill, FillPortion, Font, Padding, Theme};

use crate::app::{App, Filter, Message, Modal, SettingsTab, Update, WhenDone};
use crate::icons::{self, icon};
use crate::item::{Item, State};
use crate::style::{self, Pick};
use crate::{dialogs, update};
use crate::{format, store};

/// The sidebar's width.
const SIDEBAR_WIDTH: f32 = 236.0;

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
    let divider = container(space()).width(1).height(Fill).style(style::rule);
    let base = container(column![row![sidebar(app), divider, main_panel(app)].height(Fill), status_bar(app)])
        .width(Fill)
        .height(Fill)
        .style(style::app_background);
    let now = std::time::Instant::now();
    let theme = app.theme();
    if let Some(open) = &app.modal {
        let shown = app.motion.dialog(now);
        return match dialog(app, open, now) {
            Some(content) => modal(base, faded(&theme, shown, content), shown, true),
            None => base.into(),
        };
    }
    // The dialog that just closed, fading out. It no longer takes input.
    if let Some(closing) = &app.motion.closing {
        let shown = app.motion.dialog(now);
        let content = closing
            .as_ref()
            .and_then(|closed| dialog(app, closed, now))
            .map_or_else(|| space().into(), |content| faded(&theme, shown, content));
        return modal(base, content, shown, false);
    }
    base.into()
}

/// A dialog's contents, or none when what it shows is gone.
fn dialog<'a>(app: &'a App, modal: &'a Modal, now: std::time::Instant) -> Option<Element<'a, Message>> {
    Some(match modal {
        Modal::Add(dialog) => dialogs::add(dialog, app.existing_partial(dialog)),
        Modal::Settings(draft) => dialogs::settings(
            draft,
            app.update.status(),
            app.settings_tab,
            &app.seed_ratio_text,
            app.motion.tab(now),
            app.motion.section_height(now),
            &app.theme(),
        ),
        Modal::Report(dialog) => dialogs::report(dialog),
        Modal::Refresh(dialog) => dialogs::refresh(dialog),
        Modal::Remove { id, delete_file } => dialogs::remove(app.items.iter().find(|i| i.id == *id)?, *delete_file),
        Modal::Details(id) => dialogs::details(
            app.items.iter().find(|i| i.id == *id)?,
            app.copied.as_deref(),
            &app.item_limit_text,
            app.details_torrent.as_ref(),
        ),
        Modal::Extension(browser) => dialogs::extension(*browser, app.settings.extension_connected, app.copied.as_deref()),
        Modal::PowerCountdown { action, deadline } => {
            let left = deadline.saturating_duration_since(now);
            dialogs::power_countdown(*action, left.as_secs_f64().ceil() as u64)
        }
    })
}

/// `content` drawn with every color at `shown` (0 to 1) of its opacity, and
/// a few pixels lower until it's fully in: a dialog fading and rising in.
pub fn faded<'a>(theme: &Theme, shown: f32, content: Element<'a, Message>) -> Element<'a, Message> {
    if shown >= 1.0 {
        return content;
    }
    let lift = (1.0 - shown) * 10.0;
    container(themer(Some(style::faded_theme(theme, shown)), content).text_color(|t| style::colors(t).text))
        .padding(iced::Padding {
            top: lift,
            ..iced::Padding::ZERO
        })
        .into()
}

/// Shows `content` centered over a dimmed `base`, the dimming `shown` (0 to 1)
/// of the way in. Clicking outside closes it. While it fades out, with
/// `interactive` false, nothing in it can be clicked.
fn modal<'a>(base: impl Into<Element<'a, Message>>, content: Element<'a, Message>, shown: f32, interactive: bool) -> Element<'a, Message> {
    let layer = center(opaque(content)).style(style::backdrop(shown));
    let layer: Element<'a, Message> = if interactive {
        mouse_area(layer).on_press(Message::CloseModal).into()
    } else {
        stack![layer, mouse_area(container(space()).width(Fill).height(Fill))].into()
    };
    stack![base.into(), opaque(layer)].into()
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
    let filter_item = |filter: Filter, indent: bool| {
        let handle = match filter {
            Filter::All => &icons::FOLDER,
            Filter::Queue => &icons::CLOCK,
            Filter::Torrents => &icons::TORRENT,
            Filter::Category(Category::Compressed) => &icons::COMPRESSED,
            Filter::Category(Category::Documents) => &icons::DOCUMENT,
            Filter::Category(Category::Music) => &icons::MUSIC,
            Filter::Category(Category::Programs) => &icons::PROGRAM,
            Filter::Category(Category::Video) => &icons::VIDEO,
        };
        let item = nav_item(
            handle,
            filter.label(),
            Some(app.count(filter)),
            app.filter == filter,
            Message::Filter(filter),
        );
        if indent {
            container(item).padding(Padding { left: 14.0, ..Padding::ZERO }).into()
        } else {
            item
        }
    };
    // All, with every kind of file under it, as AB Download Manager's category card.
    let mut types = column![filter_item(Filter::All, false)].spacing(2);
    for kind in Category::ALL.map(Filter::Category) {
        types = types.push(filter_item(kind, true));
    }
    let lists = column![filter_item(Filter::Queue, false), filter_item(Filter::Torrents, false)].spacing(2);
    let cards = column![
        container(types).padding(6).width(Fill).style(style::card),
        container(lists).padding(6).width(Fill).style(style::card),
    ]
    .spacing(10);
    // Scrolls when the window is too short for every entry.
    let cards = scrollable(cards).height(Fill).spacing(4).style(style::scrollbar);

    let footer = column![
        nav_item(&icons::GLOBE, "Browser extension", None, false, Message::OpenExtension),
        nav_item(&icons::SETTINGS, "Settings", None, false, Message::OpenSettings),
        nav_item(&icons::HEART, "Support B-Downloader", None, false, Message::OpenSupport),
    ]
    .spacing(2);

    container(column![cards, space().height(12), footer])
        .padding(Padding {
            top: 14.0,
            right: 12.0,
            bottom: 12.0,
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
    let items: Vec<&Item> = app.visible().collect();
    let list: Element<'_, Message> = if app.items.is_empty() && app.filter != Filter::Torrents {
        empty_state()
    } else if items.is_empty() && app.filter == Filter::Torrents && app.count(Filter::Torrents) == 0 {
        torrents_empty_state()
    } else if items.is_empty() {
        center(muted("Nothing here.").size(14)).into()
    } else {
        scrollable(groups(&items).padding(Padding {
            top: 0.0,
            right: 12.0,
            bottom: 14.0,
            left: 0.0,
        }))
        .height(Fill)
        .style(style::scrollbar)
        .into()
    };

    let mut body = column![toolbar(app)].spacing(14);
    if app.filter == Filter::Queue {
        body = body.push(container(queue_bar(app)).padding(Padding {
            right: 12.0,
            ..Padding::ZERO
        }));
    }
    if let Some(banner) = update_banner(app) {
        body = body.push(container(banner).padding(Padding {
            right: 12.0,
            ..Padding::ZERO
        }));
    }
    body.push(list)
        .padding(Padding {
            top: 12.0,
            right: 4.0,
            bottom: 0.0,
            left: 16.0,
        })
        .width(Fill)
        .into()
}

/// Add URL and the actions for all downloads, icon over label, with search at the end.
fn toolbar(app: &App) -> Element<'_, Message> {
    let add = button(
        row![
            icon(&icons::LINK, 17.0, |c| c.text),
            text("Add URL").size(14).font(SEMIBOLD),
            container(icon(&icons::DOWNLOAD, 16.0, |c| c.on_accent)).center(30).style(style::logo),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .padding(Padding {
        top: 6.0,
        right: 6.0,
        bottom: 6.0,
        left: 14.0,
    })
    .style(style::secondary)
    .on_press(Message::OpenAdd);
    let any = |f: fn(&Item) -> bool| app.items.iter().any(f);
    let running = any(|i| i.handle.is_some());
    let stopped = any(|i| i.handle.is_none() && matches!(i.state, State::Paused | State::Failed(_)) && !i.queued);
    let queue = if app.queue_running {
        tool(&icons::PAUSE, "Stop queue", Some(Message::StopQueue))
    } else {
        tool(
            &icons::CLOCK,
            "Start queue",
            any(Item::is_waiting_in_queue).then_some(Message::StartQueue),
        )
    };
    let divider = container(space()).width(1).height(34).style(style::rule);

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
    let room = app.width - SIDEBAR_WIDTH;
    row![
        add,
        space().width(4),
        tool(&icons::PLAY, "Resume all", stopped.then_some(Message::ResumeAll)),
        tool(&icons::PAUSE, "Pause all", running.then_some(Message::PauseAll)),
        divider,
        queue,
        tool(&icons::SETTINGS, "Settings", Some(Message::OpenSettings)),
        space().width(Fill),
        search.width(if room >= 820.0 { 240 } else { 170 }),
        space().width(12),
    ]
    .spacing(2)
    .align_y(Center)
    .into()
}

/// A toolbar button: icon over label. Dimmed while there's nothing to do.
fn tool<'a>(handle: &icons::Handle, label: &'a str, message: Option<Message>) -> Element<'a, Message> {
    let pick: Pick = if message.is_some() { |c| c.text } else { |c| c.muted };
    button(
        column![icon(handle, 18.0, pick), colored(label, pick).size(12)]
            .spacing(4)
            .align_x(Center),
    )
    .padding([6, 10])
    .width(84)
    .style(style::icon_button)
    .on_press_maybe(message)
    .into()
}

/// The list by what each download needs: running ones lead in tall rows;
/// those waiting for the user, then the queue, then finished ones follow as
/// compact rows.
fn groups<'a>(items: &[&'a Item]) -> iced::widget::Column<'a, Message> {
    let running: Vec<&Item> = items.iter().copied().filter(|i| i.state.is_running()).collect();
    let needs_you: Vec<&Item> = items
        .iter()
        .copied()
        .filter(|i| !i.state.is_running() && !i.queued && matches!(i.state, State::Paused | State::Failed(_)))
        .collect();
    let queued: Vec<&Item> = items.iter().copied().filter(|i| i.is_waiting_in_queue()).collect();
    let finished: Vec<&Item> = items.iter().copied().filter(|i| i.state == State::Completed).collect();

    let mut list = column![].spacing(8);
    if !running.is_empty() {
        let speed: f64 = running.iter().map(|i| i.rate()).sum();
        list = list.push(group_heading(
            "Downloading now",
            running.len(),
            Some(format!("{} together", format::speed(speed))),
        ));
        list = list.extend(running.into_iter().map(download_card));
    }
    for (title, group) in [("Needs you", needs_you), ("Queued", queued), ("Finished", finished)] {
        if group.is_empty() {
            continue;
        }
        list = list.push(group_heading(title, group.len(), None));
        let rows = column(group.into_iter().enumerate().map(|(n, item)| compact_row(item, n > 0))).width(Fill);
        list = list.push(container(rows).width(Fill).style(style::group));
    }
    list
}

fn group_heading<'a>(title: &'a str, count: usize, note: Option<String>) -> Element<'a, Message> {
    let mut heading = row![
        text(title).size(14).font(SEMIBOLD),
        container(muted(count.to_string()).size(12)).padding([1, 8]).style(style::panel),
    ]
    .spacing(10)
    .align_y(Center);
    if let Some(note) = note {
        heading = heading.push(muted(note).size(13));
    }
    container(heading)
        .padding(Padding {
            top: 10.0,
            right: 0.0,
            bottom: 2.0,
            left: 2.0,
        })
        .into()
}

/// One download in a compact group: name and kind, what it's doing, its
/// size, and the step it waits for.
fn compact_row(item: &Item, divided: bool) -> Element<'_, Message> {
    let id = item.id;
    let (word, word_pick, detail, detail_pick) = status(item);
    let name = row![
        text(format::middle_ellipsis(&item.display_name(), 60))
            .size(14)
            .wrapping(text::Wrapping::None),
        muted(kind_of(item)).size(12),
    ]
    .spacing(10)
    .align_y(Center);
    // The size column has the sizes; the reason is what a failed row adds.
    let state = match &item.state {
        State::Failed(_) => colored(format!("{word} · {detail}"), detail_pick),
        State::Completed if item.seeding.is_some() => colored(format!("{word} · {detail}"), word_pick),
        _ => colored(word, word_pick),
    };
    let size = match (item.state == State::Completed, item.size) {
        (true, Some(size)) => format::size(size),
        (false, Some(size)) if item.downloaded > 0 => format!("{} of {}", format::size(item.downloaded), format::size(size)),
        (_, Some(size)) => format::size(size),
        (_, None) => String::new(),
    };
    let row = row![
        container(name).width(FillPortion(5)).clip(true),
        container(state.size(13).wrapping(text::Wrapping::None)).width(FillPortion(4)).clip(true),
        container(muted(size).size(13)).width(FillPortion(2)),
        row_actions(item, id),
    ]
    .spacing(12)
    .align_y(Center);
    let cell = container(row).padding([6, 12]).width(Fill);
    if divided {
        column![container(space()).width(Fill).height(1).style(style::rule), cell].into()
    } else {
        cell.into()
    }
}

/// The kind of file, as the sidebar names it, or Torrent.
fn kind_of(item: &Item) -> &'static str {
    if item.is_torrent() {
        return "Torrent";
    }
    Category::of(&item.display_name()).map_or("", |c| c.name())
}

/// The actions of a compact row: its next step in words, then Details and Remove.
fn row_actions(item: &Item, id: u64) -> Element<'_, Message> {
    let refreshable = item.size.is_some() && item.downloaded > 0 && item.file_name.is_some() && !item.is_torrent();
    let mut actions = row![].spacing(2).align_y(Center);
    match &item.state {
        State::Paused if item.queued => actions = actions.push(text_action(None, "Start now", Message::Resume(id))),
        State::Paused => actions = actions.push(text_action(Some(&icons::PLAY), "Resume", Message::Resume(id))),
        State::Failed(_) if item.certificate_failed() => {
            actions = actions.push(text_action(None, "Download anyway", Message::DownloadAnyway(id)));
        }
        State::Failed(_) if refreshable => actions = actions.push(text_action(Some(&icons::LINK), "Refresh link", Message::OpenRefresh(id))),
        State::Failed(_) => actions = actions.push(text_action(Some(&icons::RETRY), "Try again", Message::Resume(id))),
        State::Completed => {
            if item.seeding.is_some() {
                actions = actions.push(text_action(None, "Stop seeding", Message::StopSeeding(id)));
            }
            actions = actions.push(action(&icons::OPEN, "Open", Message::OpenFile(id)));
            actions = actions.push(action(&icons::FOLDER, "Show in folder", Message::ShowInFolder(id)));
        }
        _ => {}
    }
    actions = actions.push(action(&icons::INFO, "Details", Message::ShowDetails(id)));
    let remove_tip = if item.state == State::Completed { "Remove" } else { "Cancel and remove" };
    actions.push(action(&icons::TRASH, remove_tip, Message::Remove(id))).into()
}

/// The bottom bar: the controls for all downloads on the left, counts and
/// total speed on the right.
fn status_bar(app: &App) -> Element<'_, Message> {
    let running: Vec<&Item> = app.items.iter().filter(|i| i.state.is_running()).collect();
    let queued = app.items.iter().filter(|i| i.is_waiting_in_queue()).count();
    let speed: f64 = running.iter().map(|i| i.rate()).sum();
    let mut right = row![].spacing(20).align_y(Center);
    if !running.is_empty() {
        right = right.push(muted(format!("{} downloading", running.len())).size(13));
    }
    if queued > 0 {
        right = right.push(muted(format!("{queued} queued")).size(13));
    }
    right = right.push(
        row![
            icon(&icons::DOWNLOAD, 15.0, |c| c.muted),
            text(format::speed(speed)).size(13).font(SEMIBOLD)
        ]
        .spacing(6)
        .align_y(Center),
    );
    let bar = row![speed_limit(app), when_done(app), space().width(Fill), right]
        .spacing(8)
        .align_y(Center);
    column![
        container(space()).width(Fill).height(1).style(style::rule),
        container(bar).padding([6, 16]).width(Fill),
    ]
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
            format!("B-Downloader {} is available.", release.version),
            vec![
                small("What's new", Message::OpenReleasePage, false),
                small(if update::is_installed() { "Update now" } else { "Download" }, Message::UpdateNow, true),
            ],
        ),
        Update::Downloading { .. } if app.update_manual => (app.update.status(), Vec::new()),
        Update::Ready { release, .. } if app.update_manual || !app.settings.auto_update => (
            format!("B-Downloader {} is ready. Restart to finish updating.", release.version),
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

/// The Torrents tab before there are any.
fn torrents_empty_state<'a>() -> Element<'a, Message> {
    center(
        column![
            container(icon(&icons::TORRENT, 34.0, |c| c.accent))
                .center(84)
                .style(style::accent_soft(42.0)),
            space().height(8),
            text("No torrents yet").size(20).font(SEMIBOLD),
            muted("Click a magnet link in your browser, or paste one in Add download. .torrent files work too.").size(14),
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
        State::Starting if item.is_torrent() => ("Finding peers", |c| c.accent, sizes(), |c| c.muted),
        State::Starting => ("Connecting", |c| c.accent, sizes(), |c| c.muted),
        State::Downloading if item.is_torrent() && item.active_connections == 0 => ("Finding peers", |c| c.accent, sizes(), |c| c.muted),
        // Only once data has stopped, so a brief reconnect while others keep going doesn't flash.
        State::Downloading if item.problem.is_some() && item.rate() < 1024.0 => (
            "Reconnecting",
            |c| c.warning,
            capitalize(item.problem.as_deref().unwrap_or_default()),
            |c| c.muted,
        ),
        State::Downloading => ("Downloading", |c| c.accent, downloading_text(item), |c| c.muted),
        State::Verifying if item.is_torrent() => ("Checking", |c| c.accent, "Checking the files already there".into(), |c| c.muted),
        State::Verifying => ("Verifying", |c| c.accent, "Checking the checksum".into(), |c| c.muted),
        State::Paused if item.queued => ("Queued", |c| c.muted, sizes(), |c| c.muted),
        State::Paused => ("Paused", |c| c.warning, sizes(), |c| c.muted),
        State::Completed if item.seeding.is_some() => ("Seeding", |c| c.accent, seeding_text(item), |c| c.muted),
        State::Completed => (
            "Completed",
            |c| c.success,
            format!("{} · {}", item.size.map(format::size).unwrap_or_default(), item.host()),
            |c| c.muted,
        ),
        State::Failed(_) if item.certificate_failed() => (
            "Failed",
            |c| c.danger,
            "The site's security certificate isn't trusted by Windows".into(),
            |c| c.danger,
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
    let refreshable = item.size.is_some() && item.downloaded > 0 && item.file_name.is_some() && !item.is_torrent();
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
        State::Failed(_) if item.certificate_failed() => {
            actions = actions.push(text_action(None, "Download anyway", Message::DownloadAnyway(id)));
            actions = actions.push(action(&icons::RETRY, "Try again", Message::Resume(id)));
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
            if item.seeding.is_some() {
                actions = actions.push(text_action(None, "Stop seeding", Message::StopSeeding(id)));
            }
            actions = actions.push(action(&icons::OPEN, "Open", Message::OpenFile(id)));
            actions = actions.push(action(&icons::FOLDER, "Show in folder", Message::ShowInFolder(id)));
        }
        _ => {}
    }
    actions = actions.push(action(&icons::INFO, "Details", Message::ShowDetails(id)));
    let remove_tip = if item.state == State::Completed { "Remove" } else { "Cancel and remove" };
    actions = actions.push(action(&icons::TRASH, remove_tip, Message::Remove(id)));

    let top = row![
        badge(item.extension(), failed),
        container(column![name, meta].spacing(5)).width(Fill).clip(true),
        actions
    ]
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
    let rate = item.rate();
    if rate > 0.0 {
        parts.push(format::speed(rate));
    }
    if item.is_torrent() && item.upload_rate > 0.0 {
        parts.push(format!("{} up", format::speed(item.upload_rate)));
    }
    if let Some(eta) = item.eta() {
        parts.push(format::eta(eta));
    }
    if let Some(mb) = item.speed_limit_mb {
        parts.push(format!("limited to {} MB/s", format::number(mb)));
    }
    if item.is_torrent() {
        parts.extend(swarm_parts(item, item.active_connections));
        return parts.join(" · ");
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

/// A finished torrent sharing with others: how much it has given back.
fn seeding_text(item: &Item) -> String {
    let mut parts = vec![format!("Ratio {:.2}", item.ratio().unwrap_or(0.0))];
    if let Some(seeding) = &item.seeding {
        if seeding.upload_rate > 0.0 {
            parts.push(format!("{} up", format::speed(seeding.upload_rate)));
        }
        parts.extend(swarm_parts(item, seeding.peers));
    }
    parts.join(" · ")
}

/// Seeds and peers in the torrent's swarm (from its trackers), and how many
/// this PC is connected to; just the connected ones when trackers don't say.
fn swarm_parts(item: &Item, connected: usize) -> Vec<String> {
    match item.swarm {
        Some(swarm) => vec![
            format!("{} seeds", swarm.seeds),
            format!("{} peers", swarm.peers),
            format!("{connected} connected"),
        ],
        None => vec![match connected {
            1 => "1 peer".to_owned(),
            n => format!("{n} peers"),
        }],
    }
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

/// The progress bar: the gradient while it runs, the state's colour when
/// stopped. While several connections work, a thin strip under it shows each
/// part of the file, finished parts in green, so the connections fill it in.
fn progress(item: &Item) -> Element<'_, Message> {
    let pick: Pick = match item.state {
        State::Paused if item.queued => |c| c.muted,
        State::Paused => |c| c.warning,
        State::Failed(_) => |c| c.danger,
        _ => |c| c.accent,
    };
    let bar: Element<'_, Message> = if item.state.is_running() {
        progress_bar(0.0..=1.0, item.fraction()).girth(6).style(style::progress_gradient).into()
    } else {
        progress_bar(0.0..=1.0, item.fraction()).girth(6).style(style::progress(pick)).into()
    };
    match item.size {
        Some(size) if size > 0 && item.segments.len() > 1 => column![bar, segment_map(item, size)].spacing(5).into(),
        _ => bar,
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
        let pick: Pick = if done { |c| c.success } else { |c| c.track };
        container(space())
            .width(FillPortion(portion))
            .height(3)
            .style(style::segment(pick))
            .into()
    }))
    .spacing(2)
    .into()
}
