//! Colors and widget styles. Everything visual lives here, so the look can be
//! changed in one place.
//!
//! The look follows shadcn/ui on its slate scale with one blue: flat
//! surfaces, 1px borders, small radii, and colour only where it means
//! something (the primary action, progress, a download's state).

use std::sync::LazyLock;

use iced::theme::Palette;
use iced::widget::{button, container, pick_list, progress_bar, scrollable, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

pub struct Colors {
    pub background: Color,
    pub sidebar: Color,
    pub surface: Color,
    /// Hover fills, the selected sidebar entry, and muted tracks and badges.
    pub surface_hover: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
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

pub const fn alpha(color: Color, a: f32) -> Color {
    Color { a, ..color }
}

pub const DARK: Colors = Colors {
    background: rgb(0x020617),
    sidebar: rgb(0x020617),
    surface: rgb(0x0f172a),
    surface_hover: rgb(0x1e293b),
    border: rgb(0x1e293b),
    text: rgb(0xf8fafc),
    muted: rgb(0x94a3b8),
    accent: rgb(0x3b82f6),
    on_accent: rgb(0xffffff),
    success: rgb(0x22c55e),
    warning: rgb(0xf59e0b),
    danger: rgb(0xef4444),
    track: rgb(0x1e293b),
};

pub const LIGHT: Colors = Colors {
    background: rgb(0xffffff),
    sidebar: rgb(0xf8fafc),
    surface: rgb(0xffffff),
    surface_hover: rgb(0xf1f5f9),
    border: rgb(0xe2e8f0),
    text: rgb(0x0f172a),
    muted: rgb(0x64748b),
    accent: rgb(0x2563eb),
    on_accent: rgb(0xffffff),
    success: rgb(0x16a34a),
    warning: rgb(0xd97706),
    danger: rgb(0xdc2626),
    track: rgb(0xf1f5f9),
};

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

pub fn colors(theme: &Theme) -> &'static Colors {
    if theme.extended_palette().is_dark { &DARK } else { &LIGHT }
}

/// Controls: buttons, inputs, tabs.
pub const RADIUS: f32 = 6.0;
/// Cards and dialogs.
const CARD_RADIUS: f32 = 10.0;

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
    let c = colors(theme);
    container::Style {
        background: Some(c.sidebar.into()),
        border: border(c.border, 1.0, 0.0),
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

pub fn backdrop(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(alpha(Color::BLACK, 0.6).into()),
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

pub fn logo(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.accent.into()),
        border: border(Color::TRANSPARENT, 0.0, 7.0),
        ..container::Style::default()
    }
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

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => alpha(c.accent, 0.9),
        button::Status::Disabled => alpha(c.accent, 0.5),
        button::Status::Active => c.accent,
    };
    button::Style {
        background: Some(background.into()),
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

/// An outline button, as shadcn/ui's "outline" variant.
pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => c.surface_hover,
        _ => c.background,
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled { c.muted } else { c.text },
        border: border(c.border, 1.0, RADIUS),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A borderless button, as shadcn/ui's "ghost" variant: Cancel and the like.
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
/// sits on the page's own ground, as shadcn/ui's tabs (outlined rather than
/// shadowed, for the same reason as [`dialog`]).
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

/// A thin slate thumb with no track, darker while it's used.
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
