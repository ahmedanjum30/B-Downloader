//! Colors and widget styles. Everything visual lives here, so the look can be
//! changed in one place.

use std::sync::LazyLock;

use iced::theme::Palette;
use iced::widget::{button, container, progress_bar, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

pub struct Colors {
    pub background: Color,
    pub sidebar: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_hover: Color,
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
    background: rgb(0x0f1115),
    sidebar: rgb(0x14171d),
    surface: rgb(0x1a1e26),
    surface_hover: rgb(0x21262f),
    border: rgb(0x272c37),
    text: rgb(0xe8eaf0),
    muted: rgb(0x8b93a7),
    accent: rgb(0x6e7bff),
    accent_hover: rgb(0x8590ff),
    on_accent: rgb(0xffffff),
    success: rgb(0x34d399),
    warning: rgb(0xfbbf24),
    danger: rgb(0xf87171),
    track: rgb(0x272c37),
};

pub const LIGHT: Colors = Colors {
    background: rgb(0xf5f6fa),
    sidebar: rgb(0xffffff),
    surface: rgb(0xffffff),
    surface_hover: rgb(0xf3f4f9),
    border: rgb(0xe3e6ee),
    text: rgb(0x151821),
    muted: rgb(0x6b7280),
    accent: rgb(0x4f5bff),
    accent_hover: rgb(0x3f4af0),
    on_accent: rgb(0xffffff),
    success: rgb(0x10b981),
    warning: rgb(0xd97706),
    danger: rgb(0xef4444),
    track: rgb(0xe6e9f1),
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

pub const RADIUS: f32 = 10.0;

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
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn card(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface.into()),
        border: border(c.border, 1.0, 14.0),
        ..container::Style::default()
    }
}

pub fn dialog(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface.into()),
        border: border(c.border, 1.0, 16.0),
        text_color: Some(c.text),
        ..container::Style::default()
    }
}

pub fn backdrop(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(alpha(Color::BLACK, 0.55).into()),
        ..container::Style::default()
    }
}

/// A soft tinted box, used for file-type badges and status pills.
pub fn tinted(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(alpha(color, 0.14).into()),
        border: border(Color::TRANSPARENT, 0.0, radius),
        text_color: Some(color),
        ..container::Style::default()
    }
}

/// An inset panel inside a dialog.
pub fn panel(theme: &Theme) -> container::Style {
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

pub fn logo(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.accent.into()),
        border: border(Color::TRANSPARENT, 0.0, RADIUS),
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
        border: border(Color::TRANSPARENT, 0.0, 6.0),
        ..container::Style::default()
    }
}

// Buttons

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => c.accent_hover,
        button::Status::Disabled => alpha(c.accent, 0.4),
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
        border: border(Color::TRANSPARENT, 0.0, 8.0),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Sidebar entry and segmented-control option.
pub fn nav(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let background = match (selected, status) {
            (true, _) => Some(Background::Color(alpha(c.accent, 0.14))),
            (false, button::Status::Hovered | button::Status::Pressed) => Some(Background::Color(c.surface_hover)),
            _ => None,
        };
        button::Style {
            background,
            text_color: if selected { c.accent } else { c.text },
            border: border(Color::TRANSPARENT, 0.0, 8.0),
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
        background: c.background.into(),
        border: border(border_color, 1.0, RADIUS),
        icon: c.muted,
        placeholder: c.muted,
        value: c.text,
        selection: alpha(c.accent, 0.35),
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
