//! Colors and widget styles. Everything visual lives here, so the look can be
//! changed in one place.
//!
//! The look follows AB Download Manager: charcoal panels with 1px borders and
//! generous rounding, and one blue-to-violet gradient where something moves
//! or is the main action (progress, Add URL, the primary button). Other
//! colour only where it means something: a download's state.

use std::sync::LazyLock;

use iced::gradient::Linear;
use iced::theme::Palette;
use iced::widget::slider::{self as slider_widget, HandleShape};
use iced::widget::{button, container, pick_list, progress_bar, scrollable, text_input};
use iced::{Background, Border, Color, Degrees, Gradient, Shadow, Theme};

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

pub static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| theme_from("BDownloader Dark", &DARK));
pub static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| theme_from("BDownloader Light", &LIGHT));

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
    }
}

/// Each theme at every fade step, from nearly see-through to nearly opaque:
/// a fading dialog draws with these, so all its colors fade together.
static FADES: LazyLock<[Vec<(Theme, Colors)>; 2]> = LazyLock::new(|| {
    let steps = |name: &'static str, c: &Colors| {
        (0..FADE_STEPS)
            .map(|i| {
                let colors = faded(c, i as f32 / FADE_STEPS as f32);
                (theme_from(name, &colors), colors)
            })
            .collect()
    };
    [steps("BDownloader Dark", &DARK), steps("BDownloader Light", &LIGHT)]
});

fn fade_step(a: f32) -> Option<usize> {
    (a < 1.0).then(|| ((a.max(0.0) * FADE_STEPS as f32) as usize).min(FADE_STEPS - 1))
}

/// `theme` with every color at `a` of its opacity, for a fading dialog.
pub fn faded_theme(theme: &Theme, a: f32) -> Theme {
    let dark = theme.extended_palette().is_dark;
    match fade_step(a) {
        Some(step) => FADES[usize::from(!dark)][step].0.clone(),
        None => {
            if dark {
                DARK_THEME.clone()
            } else {
                LIGHT_THEME.clone()
            }
        }
    }
}

pub fn colors(theme: &Theme) -> &'static Colors {
    let dark = theme.extended_palette().is_dark;
    match fade_step(theme.palette().background.a) {
        Some(step) => &FADES[usize::from(!dark)][step].1,
        None if dark => &DARK,
        None => &LIGHT,
    }
}

/// Controls: buttons, inputs, tabs.
pub const RADIUS: f32 = 8.0;
/// Cards, panels and dialogs.
const CARD_RADIUS: f32 = 12.0;

fn border(color: Color, width: f32, radius: f32) -> Border {
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
        border: border(c.border, 1.0, CARD_RADIUS),
        ..container::Style::default()
    }
}

/// A card that needs the user: its border takes the danger colour.
pub fn card_alert(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: border(alpha(c.danger, 0.45), 1.0, CARD_RADIUS),
        ..card(theme)
    }
}

/// No shadow: the software renderer repaints only what changes under the
/// mouse, and a blurred shadow repainted in pieces smears over the dialog.
pub fn dialog(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        border: border(c.border, 1.0, CARD_RADIUS),
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
            background: Some(c.surface_hover.into()),
            border: border(Color::TRANSPARENT, 0.0, 8.0),
            text_color: Some(if failed { c.danger } else { c.muted }),
            ..container::Style::default()
        }
    }
}

/// A status pill: outlined, its dot carries the colour.
pub fn pill(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        border: border(c.border, 1.0, 999.0),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

/// A muted inset: the track behind segmented tabs, and read-only boxes.
pub fn panel(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface_hover.into()),
        border: border(Color::TRANSPARENT, 0.0, RADIUS + 2.0),
        ..container::Style::default()
    }
}

/// An outlined box around a group of controls, like an outline button.
pub fn outline(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        border: border(c.border, 1.0, RADIUS),
        ..container::Style::default()
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
        border: border(Color::TRANSPARENT, 0.0, 8.0),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
        border: border(c.border, 1.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Sidebar entry: the selected one sits on a muted fill.
pub fn nav(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let background = match (selected, status) {
            (true, _) | (false, button::Status::Hovered | button::Status::Pressed) => Some(Background::Color(c.surface_hover)),
            _ => None,
        };
        button::Style {
            background,
            text_color: if selected { c.text } else { c.muted },
            border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
            border: border(if selected { c.border } else { Color::TRANSPARENT }, 1.0, RADIUS),
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
        border: border(border_color, 1.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
            border: border(Color::TRANSPARENT, 0.0, 3.0),
        }
    }
}

/// A running download's bar, in the gradient.
pub fn progress_gradient(theme: &Theme) -> progress_bar::Style {
    let c = colors(theme);
    progress_bar::Style {
        background: c.track.into(),
        bar: gradient(c),
        border: border(Color::TRANSPARENT, 0.0, 3.0),
    }
}
