//! ANSI colours follow the user's terminal palette, on light and dark themes.
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders},
};
pub const ACCENT: Color = Color::Cyan;
pub const MUTED: Color = Color::DarkGray;
pub const SEARCH: Color = Color::Magenta;
pub const WARN: Color = Color::Yellow;
pub fn badge(color: Color) -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}
pub fn block(title: String, focused: bool) -> Block<'static> {
    let color = if focused { ACCENT } else { MUTED };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
}

/// Installation intent is evidence, not a package health rating.
pub fn reason(reason: xpac_core::InstallReason) -> Style {
    match reason {
        xpac_core::InstallReason::Explicit => Style::default().fg(ACCENT),
        xpac_core::InstallReason::Dependency => Style::default(),
        xpac_core::InstallReason::Unknown => Style::default().fg(WARN),
    }
}
