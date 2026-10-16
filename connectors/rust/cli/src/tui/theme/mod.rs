//! Colors and symbols of the interface (doc 34, TUI-FRM-09): `NO_COLOR` takes the colors away and
//! `TABA_CLI_ASCII` (or a locale that is not UTF-8) swaps the symbols for ASCII. A state is always a word
//! **and** a symbol, never only a color.

use ratatui::style::{Color, Modifier, Style};

use crate::tui::api::Severity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub color: bool,
    pub ascii: bool,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            color: true,
            ascii: false,
        }
    }
}

impl Theme {
    /// `NO_COLOR` counts when it is not empty (the convention). A locale that does not say UTF-8 means ASCII;
    /// with no locale at all (Windows) the terminal is assumed to speak UTF-8.
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Theme {
        let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|name| env(name).filter(|v| !v.is_empty()))
            .is_none_or(|locale| {
                let upper = locale.to_ascii_uppercase();
                upper.contains("UTF-8") || upper.contains("UTF8")
            });
        Theme {
            color: !set("NO_COLOR"),
            ascii: set("TABA_CLI_ASCII") || !utf8,
        }
    }

    fn paint(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    pub fn ok(self) -> Style {
        self.paint(Color::Green)
    }

    pub fn warning(self) -> Style {
        self.paint(Color::Yellow)
    }

    pub fn critical(self) -> Style {
        self.paint(Color::Red).add_modifier(Modifier::BOLD)
    }

    pub fn dim(self) -> Style {
        if self.color {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        }
    }

    pub fn accent(self) -> Style {
        self.paint(Color::Cyan)
    }

    pub fn title(self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    pub fn key(self) -> Style {
        self.paint(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    /// The selected row: a bold band over dark gray, not `reversed` (TUI-FRM-09).
    pub fn selected(self) -> Style {
        let base = Style::default().add_modifier(Modifier::BOLD);
        if self.color {
            base.bg(Color::Indexed(238))
        } else {
            base.add_modifier(Modifier::UNDERLINED)
        }
    }

    pub fn severity(self, severity: Severity) -> Style {
        match severity {
            Severity::Ok => self.ok(),
            Severity::Warning => self.warning(),
            Severity::Critical => self.critical(),
        }
    }

    /// The symbol before a state word.
    pub fn icon(self, severity: Severity) -> &'static str {
        match (severity, self.ascii) {
            (Severity::Ok, false) => "✔",
            (Severity::Warning, false) => "▲",
            (Severity::Critical, false) => "✖",
            (Severity::Ok, true) => "[ok]",
            (Severity::Warning, true) => "[!]",
            (Severity::Critical, true) => "[x]",
        }
    }

    pub fn selector(self) -> &'static str {
        if self.ascii {
            "> "
        } else {
            "▶ "
        }
    }

    pub fn offline(self) -> &'static str {
        if self.ascii {
            "[x]"
        } else {
            "✖"
        }
    }

    pub fn refresh(self) -> &'static str {
        if self.ascii {
            "~"
        } else {
            "⟳"
        }
    }

    /// The glyph of a bar of `ratio` (0 to 1) across `width` cells.
    pub fn bar(self, ratio: f64, width: usize) -> String {
        let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
        let (on, off) = if self.ascii {
            ('#', '-')
        } else {
            ('█', '░')
        };
        std::iter::repeat_n(on, filled)
            .chain(std::iter::repeat_n(off, width - filled))
            .collect()
    }

    /// The marker of each chart line, distinct per line (TUI-MON-02).
    pub fn marker(self, index: usize) -> &'static str {
        const FANCY: [&str; 4] = ["●", "◆", "▲", "■"];
        const PLAIN: [&str; 4] = ["*", "+", "o", "#"];
        if self.ascii {
            PLAIN[index % 4]
        } else {
            FANCY[index % 4]
        }
    }

    /// The color of the line of a node in a chart; the marker tells them apart without color.
    pub fn series(self, index: usize) -> Style {
        const COLORS: [Color; 6] = [
            Color::Cyan,
            Color::Yellow,
            Color::Green,
            Color::Magenta,
            Color::LightBlue,
            Color::LightRed,
        ];
        self.paint(COLORS[index % COLORS.len()])
    }

    pub fn arrow_up(self) -> &'static str {
        if self.ascii {
            "^"
        } else {
            "↑"
        }
    }
}

#[cfg(test)]
mod tests;
