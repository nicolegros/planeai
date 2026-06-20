//! Centered modal overlay panel with standard styling.

use iced::widget::{container, stack};
use iced::{Element, Length, Theme};

use crate::theme::PlaneAiTheme;

/// Wraps content in a centered modal overlay panel with theme-aware styling.
pub fn modal_overlay<'a, M: 'a>(
    content: iced::widget::Column<'a, M>,
    base: Element<'a, M>,
    theme: &PlaneAiTheme,
) -> Element<'a, M> {
    let bg = theme.panel_bg();
    let border_color = theme.border();
    let panel = container(content)
        .width(Length::Fixed(500.0))
        .padding(16)
        .style(move |_: &Theme| container::Style {
            background: Some(bg.into()),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });
    let overlay = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    stack![base, overlay].into()
}
