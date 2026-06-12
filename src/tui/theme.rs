#![allow(dead_code)]

use ratatui::style::{Color, Style};
use rustui::style::{ColorToken, Design, Palette, Role, style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Safe,
    Unsafe,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub design: Design,
}

impl Theme {
    pub fn style(&self, role: Role) -> Style {
        self.design.role_style(role)
    }

    pub fn selector(&self, selector: &str) -> Style {
        self.design.style(selector)
    }

    pub fn color(&self, token: ColorToken) -> Color {
        self.design.color(token)
    }

    pub fn palette(&self) -> Palette {
        self.design.palette
    }
}

impl ThemeKind {
    pub fn from_safe_mode(safe_mode: bool) -> Self {
        if safe_mode { Self::Safe } else { Self::Unsafe }
    }

    pub fn theme(self) -> Theme {
        let (name, palette) = match self {
            Self::Safe => (
                "safe",
                Palette {
                    surface0: Color::Rgb(4, 18, 12),
                    surface1: Color::Rgb(8, 32, 22),
                    surface2: Color::Rgb(13, 45, 31),
                    border: Color::Rgb(24, 130, 82),
                    text: Color::Rgb(225, 247, 235),
                    muted: Color::Rgb(131, 179, 154),
                    accent: Color::Green,
                    accent_low: Color::Rgb(0, 75, 45),
                    success: Color::Green,
                    warning: Color::LightYellow,
                    danger: Color::LightRed,
                    selection: Color::Rgb(26, 91, 62),
                },
            ),
            Self::Unsafe => (
                "unsafe",
                Palette {
                    surface0: Color::Rgb(25, 8, 10),
                    surface1: Color::Rgb(49, 15, 19),
                    surface2: Color::Rgb(62, 20, 25),
                    border: Color::Rgb(183, 53, 59),
                    text: Color::Rgb(255, 230, 230),
                    muted: Color::Rgb(207, 143, 145),
                    accent: Color::Red,
                    accent_low: Color::Rgb(93, 22, 25),
                    success: Color::Green,
                    warning: Color::LightYellow,
                    danger: Color::LightYellow,
                    selection: Color::Rgb(103, 31, 36),
                },
            ),
        };

        Theme {
            name,
            design: base_design(palette),
        }
    }
}

fn base_design(palette: Palette) -> Design {
    Design::new(palette)
        .role(
            Role::AppBackground,
            style().fg(palette.text).bg(palette.surface0),
        )
        .role(Role::Header, style().fg(palette.text).bg(palette.surface1))
        .role(
            Role::HeaderBrand,
            style().fg(palette.surface0).bg(palette.accent).bold(),
        )
        .role(
            Role::HeaderMode,
            style().fg(palette.text).bg(palette.accent_low).bold(),
        )
        .role(Role::Panel, style().fg(palette.text).bg(palette.surface1))
        .role(
            Role::PanelFocused,
            style().fg(palette.accent).bg(palette.surface1),
        )
        .role(Role::Text, style().fg(palette.text).bg(palette.surface1))
        .role(
            Role::TextMuted,
            style().fg(palette.muted).bg(palette.surface1),
        )
        .role(
            Role::ListItem,
            style().fg(palette.text).bg(palette.surface1),
        )
        .role(
            Role::ListItemSelected,
            style().fg(palette.text).bg(palette.selection).bold(),
        )
        .role(Role::PromptPrefix, style().fg(palette.accent).bold())
        .role(Role::PromptInput, style().fg(palette.text))
        .role(Role::Footer, style().fg(palette.text).bg(palette.surface1))
        .role(Role::Status, style().fg(palette.muted))
        .role(Role::StatusDanger, style().fg(palette.danger).bold())
        .role(Role::Badge, style().fg(palette.text).bg(palette.accent_low))
        .role(
            Role::TableHeader,
            style().fg(palette.surface0).bg(palette.accent).bold(),
        )
        .role(
            Role::TableCell,
            style().fg(palette.text).bg(palette.surface1),
        )
        .role(Role::Input, style().fg(palette.text).bg(palette.surface1))
        .role(
            Role::InputFocused,
            style().fg(palette.text).bg(palette.surface2).underlined(),
        )
        .selector("browser.schema", style().fg(palette.muted))
        .selector("select.active.border", style().fg(palette.accent))
        .selector("select.border", style().fg(palette.border))
        .selector("dropdown.border", style().fg(palette.accent))
}
