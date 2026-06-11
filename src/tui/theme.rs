use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Safe,
    Unsafe,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub accent_dark: Color,
    pub background: Color,
    pub panel: Color,
    pub border: Color,
    pub selected: Color,
    pub text: Color,
    pub muted: Color,
    pub danger: Color,
}

impl ThemeKind {
    pub fn from_safe_mode(safe_mode: bool) -> Self {
        if safe_mode { Self::Safe } else { Self::Unsafe }
    }

    pub fn theme(self) -> Theme {
        match self {
            Self::Safe => Theme {
                accent: Color::Green,
                accent_dark: Color::Rgb(0, 75, 45),
                background: Color::Rgb(4, 18, 12),
                panel: Color::Rgb(8, 32, 22),
                border: Color::Rgb(24, 130, 82),
                selected: Color::Rgb(26, 91, 62),
                text: Color::Rgb(225, 247, 235),
                muted: Color::Rgb(131, 179, 154),
                danger: Color::LightRed,
            },
            Self::Unsafe => Theme {
                accent: Color::Red,
                accent_dark: Color::Rgb(93, 22, 25),
                background: Color::Rgb(25, 8, 10),
                panel: Color::Rgb(49, 15, 19),
                border: Color::Rgb(183, 53, 59),
                selected: Color::Rgb(103, 31, 36),
                text: Color::Rgb(255, 230, 230),
                muted: Color::Rgb(207, 143, 145),
                danger: Color::LightYellow,
            },
        }
    }
}
