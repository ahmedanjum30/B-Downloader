//! Colors and widget styles. Everything visual lives here, so the look can be
//! changed in one place.
//!
//! The look follows AB Download Manager: charcoal panels with 1px borders and
//! generous rounding, and one blue-to-violet gradient where something moves
//! or is the main action (progress, Add URL, the primary button). Other
//! colour only where it means something: a download's state.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use iced::border::Radius;
use iced::font::{Family, Weight};
use iced::gradient::Linear;
use iced::theme::Palette;
use iced::widget::slider::{self as slider_widget, HandleShape};
use iced::widget::{button, container, pick_list, progress_bar, scrollable, text_input};
use iced::{Background, Border, Color, Degrees, Font, Gradient, Shadow, Theme};

/// The app's looks: the two standard ones and the two hand-drawn ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Dark,
    Light,
    /// Graphite pencil on cream sketchbook paper.
    Graphite,
    /// Chalk on a slate board.
    Chalk,
}

impl Look {
    const ALL: [Look; 4] = [Look::Dark, Look::Light, Look::Graphite, Look::Chalk];

    pub fn colors(self) -> &'static Colors {
        match self {
            Look::Dark => &DARK,
            Look::Light => &LIGHT,
            Look::Graphite => &GRAPHITE,
            Look::Chalk => &CHALK,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Look::Dark => "BDownloader Dark",
            Look::Light => "BDownloader Light",
            Look::Graphite => "BDownloader Graphite",
            Look::Chalk => "BDownloader Chalk",
        }
    }

    pub fn theme(self) -> Theme {
        THEMES[self as usize].clone()
    }

    /// Makes this the look the views take their fonts from. Styles get the
    /// theme passed in; text gets its font when the view is built.
    pub fn apply(self) {
        CURRENT.store(self as usize, Ordering::Relaxed);
    }

    pub fn current() -> Look {
        Look::ALL[CURRENT.load(Ordering::Relaxed)]
    }

    /// The look a theme (or one of its faded copies) belongs to: each has its
    /// own window ground.
    fn of(theme: &Theme) -> Look {
        let ground = theme.palette().background;
        let same = |c: Color| (c.r - ground.r).abs() < 0.002 && (c.g - ground.g).abs() < 0.002 && (c.b - ground.b).abs() < 0.002;
        Look::ALL
            .into_iter()
            .find(|look| same(look.colors().background))
            .unwrap_or(if theme.extended_palette().is_dark { Look::Dark } else { Look::Light })
    }
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub struct Colors {
    pub background: Color,
    pub sidebar: Color,
    pub surface: Color,
    /// Hover fills, the selected sidebar entry, and muted tracks and badges.
    pub surface_hover: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    /// Where the gradient starts; also the accent on its own (focus, links).
    pub accent: Color,
    /// Where the gradient ends.
    pub accent_end: Color,
    pub on_accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub track: Color,
    /// Width of drawn outlines (cards, inputs, outlined buttons).
    pub line: f32,
    /// Corners of controls and of cards. The hand-drawn looks give each
    /// corner its own radius, so boxes look drawn rather than ruled.
    pub radius: Radius,
    pub card_radius: Radius,
    /// Hand-drawn: outlines where the standard looks use fills (the selected
    /// sidebar entry, file badges, progress tracks).
    pub sketch: bool,
    /// Body text, and labels, buttons and headings.
    pub font: Font,
    pub strong: Font,
}

/// The standard looks' font: the system's UI font (see `view::FONT`).
pub const SYSTEM_FONT: Font = Font::with_name(if cfg!(target_os = "macos") { "Helvetica Neue" } else { "Segoe UI" });

const fn corners(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Radius {
    Radius {
        top_left,
        top_right,
        bottom_right,
        bottom_left,
    }
}

const fn round(r: f32) -> Radius {
    corners(r, r, r, r)
}

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// The color at `a` of its opacity (all theme colors start fully opaque, so
/// this is `a`, except while a dialog fades and its colors are see-through).
pub const fn alpha(color: Color, a: f32) -> Color {
    Color { a: color.a * a, ..color }
}

pub const DARK: Colors = Colors {
    background: rgb(0x1c1c21),
    sidebar: rgb(0x1c1c21),
    surface: rgb(0x24242b),
    surface_hover: rgb(0x2c2c34),
    border: rgb(0x34343d),
    text: rgb(0xe6e6ea),
    muted: rgb(0x8d8d98),
    accent: rgb(0x4a8df6),
    accent_end: rgb(0xa35bf7),
    on_accent: rgb(0xffffff),
    success: rgb(0x3fb950),
    warning: rgb(0xe3a008),
    danger: rgb(0xf85149),
    track: rgb(0x2c2c34),
    line: 1.0,
    radius: round(RADIUS),
    card_radius: round(CARD_RADIUS),
    sketch: false,
    font: SYSTEM_FONT,
    strong: Font {
        weight: Weight::Semibold,
        ..SYSTEM_FONT
    },
};

pub const LIGHT: Colors = Colors {
    background: rgb(0xf4f4f6),
    sidebar: rgb(0xf4f4f6),
    surface: rgb(0xffffff),
    surface_hover: rgb(0xebebef),
    border: rgb(0xdcdce2),
    text: rgb(0x1c1c21),
    muted: rgb(0x66666f),
    accent: rgb(0x3b7ce8),
    accent_end: rgb(0x9147ee),
    on_accent: rgb(0xffffff),
    success: rgb(0x1f9d3a),
    warning: rgb(0xb87500),
    danger: rgb(0xd93a33),
    track: rgb(0xebebef),
    ..DARK
};

/// Graphite pencil on cream sketchbook paper, with blue ballpoint for what
/// moves and the main action.
pub const GRAPHITE: Colors = Colors {
    background: rgb(0xf4efe1),
    sidebar: rgb(0xf4efe1),
    surface: rgb(0xfbf8ef),
    surface_hover: rgb(0xebe4d2),
    border: rgb(0x2b2a26),
    text: rgb(0x2b2a26),
    muted: rgb(0x5c584e),
    accent: rgb(0x2b57a6),
    accent_end: rgb(0x2b57a6),
    on_accent: rgb(0xffffff),
    success: rgb(0x2f7a3a),
    warning: rgb(0x94590a),
    danger: rgb(0xb3261e),
    track: rgb(0xe4ddca),
    line: 1.6,
    radius: corners(9.0, 4.0, 8.0, 3.0),
    card_radius: corners(14.0, 5.0, 12.0, 6.0),
    sketch: true,
    font: Font::with_name("Patrick Hand"),
    strong: Font {
        family: Family::Name("Kalam"),
        weight: Weight::Bold,
        ..Font::DEFAULT
    },
};

/// Chalk on a slate board: the dark hand-drawn look.
pub const CHALK: Colors = Colors {
    background: rgb(0x262b28),
    sidebar: rgb(0x262b28),
    surface: rgb(0x2b312d),
    surface_hover: rgb(0x363d38),
    // Chalk at 55% on the slate.
    border: rgb(0x93948d),
    text: rgb(0xece9df),
    muted: rgb(0xb9b7ac),
    accent: rgb(0x8cc4f0),
    accent_end: rgb(0x8cc4f0),
    on_accent: rgb(0x1d2220),
    success: rgb(0xa3d9a0),
    warning: rgb(0xf2c46b),
    danger: rgb(0xf4978e),
    track: rgb(0x363d38),
    line: 1.5,
    radius: corners(7.0, 3.0, 8.0, 3.0),
    card_radius: corners(12.0, 4.0, 10.0, 5.0),
    sketch: true,
    font: Font::with_name("Gochi Hand"),
    strong: Font::with_name("Gochi Hand"),
};

/// The blue-to-violet gradient, left to right.
pub fn gradient(c: &Colors) -> Background {
    Gradient::Linear(Linear::new(Degrees(90.0)).add_stop(0.0, c.accent).add_stop(1.0, c.accent_end)).into()
}

fn theme_from(name: &'static str, c: &Colors) -> Theme {
    Theme::custom(
        name,
        Palette {
            background: c.background,
            text: c.text,
            primary: c.accent,
            success: c.success,
            warning: c.warning,
            danger: c.danger,
        },
    )
}

static THEMES: LazyLock<[Theme; 4]> = LazyLock::new(|| Look::ALL.map(|look| theme_from(look.name(), look.colors())));

/// Steps a fading dialog's colors go through.
const FADE_STEPS: usize = 24;

fn faded(c: &Colors, a: f32) -> Colors {
    Colors {
        background: alpha(c.background, a),
        sidebar: alpha(c.sidebar, a),
        surface: alpha(c.surface, a),
        surface_hover: alpha(c.surface_hover, a),
        border: alpha(c.border, a),
        text: alpha(c.text, a),
        muted: alpha(c.muted, a),
        accent: alpha(c.accent, a),
        accent_end: alpha(c.accent_end, a),
        on_accent: alpha(c.on_accent, a),
        success: alpha(c.success, a),
        warning: alpha(c.warning, a),
        danger: alpha(c.danger, a),
        track: alpha(c.track, a),
        ..*c
    }
}

/// Each theme at every fade step, from nearly see-through to nearly opaque:
/// a fading dialog draws with these, so all its colors fade together.
static FADES: LazyLock<[Vec<(Theme, Colors)>; 4]> = LazyLock::new(|| {
    Look::ALL.map(|look| {
        (0..FADE_STEPS)
            .map(|i| {
                let colors = faded(look.colors(), i as f32 / FADE_STEPS as f32);
                (theme_from(look.name(), &colors), colors)
            })
            .collect()
    })
});

fn fade_step(a: f32) -> Option<usize> {
    (a < 1.0).then(|| ((a.max(0.0) * FADE_STEPS as f32) as usize).min(FADE_STEPS - 1))
}

/// `theme` with every color at `a` of its opacity, for a fading dialog.
pub fn faded_theme(theme: &Theme, a: f32) -> Theme {
    let look = Look::of(theme);
    match fade_step(a) {
        Some(step) => FADES[look as usize][step].0.clone(),
        None => look.theme(),
    }
}

pub fn colors(theme: &Theme) -> &'static Colors {
    let look = Look::of(theme);
    match fade_step(theme.palette().background.a) {
        Some(step) => &FADES[look as usize][step].1,
        None => look.colors(),
    }
}

/// Controls: buttons, inputs, tabs.
pub const RADIUS: f32 = 8.0;
/// Cards, panels and dialogs.
const CARD_RADIUS: f32 = 12.0;

fn border(color: Color, width: f32, radius: impl Into<Radius>) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

// Containers

pub fn app_background(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

pub fn sidebar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors(theme).sidebar.into()),
        ..container::Style::default()
    }
}

/// A 1px line between regions.
pub fn rule(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(colors(theme).border.into()),
        ..container::Style::default()
    }
}

pub fn card(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface.into()),
        border: border(c.border, c.line, c.card_radius),
        ..container::Style::default()
    }
}

/// A card that needs the user: its border takes the danger colour.
pub fn card_alert(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: border(alpha(c.danger, 0.45), c.line, c.card_radius),
        ..card(theme)
    }
}

/// No shadow: the software renderer repaints only what changes under the
/// mouse, and a blurred shadow repainted in pieces smears over the dialog.
pub fn dialog(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        border: border(c.border, c.line, c.card_radius),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

/// A dialog that is a window of its own: the window's ground, no card edge.
pub fn dialog_window(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

/// The dimmed window behind a dialog, `shown` (0 to 1) of the way in.
pub fn backdrop(shown: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(Color::BLACK, 0.6 * shown).into()),
        ..container::Style::default()
    }
}

/// A soft tinted box, used for the empty state's icon and notices.
pub fn tinted(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(color, 0.12).into()),
        border: border(Color::TRANSPARENT, 0.0, radius),
        text_color: Some(color),
        ..container::Style::default()
    }
}

/// The file-type badge: a quiet square, red when the download failed.
pub fn file_badge(failed: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let c = colors(theme);
        container::Style {
            // Drawn as an outline in the hand-drawn looks.
            background: (!c.sketch).then_some(c.surface_hover.into()),
            border: if c.sketch {
                border(if failed { c.danger } else { c.border }, c.line, c.radius)
            } else {
                border(Color::TRANSPARENT, 0.0, 8.0)
            },
            text_color: Some(if failed {
                c.danger
            } else if c.sketch {
                c.text
            } else {
                c.muted
            }),
            ..container::Style::default()
        }
    }
}

/// A status pill: outlined, its dot carries the colour.
pub fn pill(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: border(c.border, c.line, 999.0),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

/// A muted inset: the track behind segmented tabs, and read-only boxes.
pub fn panel(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface_hover.into()),
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        ..container::Style::default()
    }
}

/// An outlined box around a group of controls, like an outline button.
pub fn outline(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        border: border(c.border, c.line, c.radius),
        ..container::Style::default()
    }
}

/// A small sample of another look, in its own paper, line and ink, for the
/// Themes section.
pub fn swatch(look: Look) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let c = look.colors();
        container::Style {
            background: Some(c.background.into()),
            border: border(c.border, c.line, c.radius),
            text_color: Some(c.text),
            ..container::Style::default()
        }
    }
}

/// Picks one of the theme's colors.
pub type Pick = fn(&Colors) -> Color;

pub fn segment(pick: Pick) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(pick(colors(theme)).into()),
        border: border(Color::TRANSPARENT, 0.0, 2.0),
        ..container::Style::default()
    }
}

/// A dot in one of the theme's colours, for status pills.
pub fn dot(pick: Pick) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(pick(colors(theme)).into()),
        border: border(Color::TRANSPARENT, 0.0, 999.0),
        ..container::Style::default()
    }
}

/// The gradient tile: the logo and the Add URL button's icon.
pub fn logo(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(gradient(c)),
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        text_color: Some(c.on_accent),
        ..container::Style::default()
    }
}

/// A group of compact rows (Needs you, Queued, Finished): one bordered panel.
pub fn group(theme: &Theme) -> container::Style {
    card(theme)
}

/// A soft circle or box in the theme's accent color.
pub fn accent_soft(radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| tinted(colors(theme).accent, radius)(theme)
}

pub fn tooltip(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.text.into()),
        text_color: Some(c.background),
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        ..container::Style::default()
    }
}

// Buttons

/// The main action, filled with the gradient.
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Disabled => alpha(c.accent, 0.45).into(),
        button::Status::Hovered | button::Status::Pressed => Gradient::Linear(
            Linear::new(Degrees(90.0))
                .add_stop(0.0, alpha(c.accent, 0.88))
                .add_stop(1.0, alpha(c.accent_end, 0.88)),
        )
        .into(),
        button::Status::Active => gradient(c),
    };
    button::Style {
        background: Some(background),
        text_color: if status == button::Status::Disabled {
            alpha(c.on_accent, 0.7)
        } else {
            c.on_accent
        },
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// For actions that delete something.
pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => alpha(c.danger, 0.9),
        _ => c.danger,
    };
    button::Style {
        background: Some(background.into()),
        text_color: c.on_accent,
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A raised button on a panel: Add URL, Browse, row actions with words.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => c.surface_hover,
        _ => c.surface,
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled { c.muted } else { c.text },
        border: border(c.border, c.line, c.radius),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A borderless button: Cancel, toolbar buttons and the like.
pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(c.surface_hover)),
        text_color: if hovered { c.text } else { c.muted },
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Square icon button in a download row.
pub fn icon_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(c.surface_hover)),
        _ => None,
    };
    button::Style {
        background,
        text_color: c.text,
        border: border(Color::TRANSPARENT, 0.0, c.radius),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Sidebar entry: the selected one sits on a muted fill.
pub fn nav(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let background = match (selected, status) {
            // The hand-drawn looks circle the selected entry instead.
            (true, _) if c.sketch => None,
            (true, _) | (false, button::Status::Hovered | button::Status::Pressed) => Some(Background::Color(c.surface_hover)),
            _ => None,
        };
        button::Style {
            background,
            text_color: if selected { c.text } else { c.muted },
            border: if selected && c.sketch {
                border(c.border, c.line, c.radius)
            } else {
                border(Color::TRANSPARENT, 0.0, c.radius)
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// One option of a segmented control on a [`panel`] track: the chosen one
/// sits on the page's own ground, outlined.
pub fn tab(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: selected.then_some(Background::Color(c.background)),
            text_color: if selected || hovered { c.text } else { c.muted },
            border: border(if selected { c.border } else { Color::TRANSPARENT }, c.line, c.radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// Inputs

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = colors(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => c.accent,
        text_input::Status::Hovered => alpha(c.muted, 0.6),
        _ => c.border,
    };
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: border(border_color, c.line, c.radius),
        icon: c.muted,
        placeholder: c.muted,
        value: c.text,
        selection: alpha(c.accent, 0.35),
    }
}

/// A dropdown sitting inside an [`outline`] box: no frame of its own.
pub fn bare_pick_list(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = colors(theme);
    let hovered = matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. });
    pick_list::Style {
        text_color: c.text,
        placeholder_color: c.muted,
        handle_color: if hovered { c.text } else { c.muted },
        background: Color::TRANSPARENT.into(),
        border: border(Color::TRANSPARENT, 0.0, c.radius),
    }
}

/// A thin thumb with no track, brighter while it's used.
pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let c = colors(theme);
    let busy = !matches!(status, scrollable::Status::Active { .. });
    let rail = scrollable::Rail {
        background: None,
        border: border(Color::TRANSPARENT, 0.0, 0.0),
        scroller: scrollable::Scroller {
            background: (if busy { c.muted } else { c.border }).into(),
            border: border(Color::TRANSPARENT, 0.0, 999.0),
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        ..scrollable::default(theme, status)
    }
}

/// A thin rail, filled with the gradient up to a white knob that grows a
/// little under the mouse and while dragged.
pub fn slider(theme: &Theme, status: slider_widget::Status) -> slider_widget::Style {
    let c = colors(theme);
    let busy = !matches!(status, slider_widget::Status::Active);
    slider_widget::Style {
        rail: slider_widget::Rail {
            backgrounds: (gradient(c), c.track.into()),
            width: 3.0,
            border: border(Color::TRANSPARENT, 0.0, 2.0),
        },
        handle: slider_widget::Handle {
            shape: HandleShape::Circle {
                radius: if busy { 8.0 } else { 7.0 },
            },
            background: c.on_accent.into(),
            border_width: 1.0,
            // Fades with the dialog, like the theme's own colors.
            border_color: alpha(Color::BLACK, 0.3 * c.text.a),
        },
    }
}

pub fn progress(pick: Pick) -> impl Fn(&Theme) -> progress_bar::Style {
    move |theme| {
        let c = colors(theme);
        progress_bar::Style {
            background: c.track.into(),
            bar: pick(c).into(),
            border: track_border(c),
        }
    }
}

/// A running download's bar, in the gradient.
pub fn progress_gradient(theme: &Theme) -> progress_bar::Style {
    let c = colors(theme);
    progress_bar::Style {
        background: c.track.into(),
        bar: gradient(c),
        border: track_border(c),
    }
}

/// A progress track's edge: drawn in pencil or chalk in the hand-drawn looks.
fn track_border(c: &Colors) -> Border {
    if c.sketch {
        border(c.border, 1.0, corners(4.0, 2.0, 4.0, 2.0))
    } else {
        border(Color::TRANSPARENT, 0.0, 3.0)
    }
}
