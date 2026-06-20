//! Title bar — macOS-style custom title bar with breadcrumb and drag-to-move.

use iced::widget::{container, mouse_area, row, text, Space};
use iced::{Element, Font, Length, Theme};

use crate::theme::PlaneAiTheme;

/// Render the title bar row.
///
/// Layout: [72px traffic-light padding] [project / session] [divider] [flex space] [12px right pad]
pub fn view<'a, Message: Clone + 'a>(
    project_name: Option<&str>,
    session_name: Option<&str>,
    theme: &'a PlaneAiTheme,
    on_drag: Message,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::new();

    // Left padding for macOS traffic lights
    items.push(Space::new().width(Length::Fixed(72.0)).into());

    // Breadcrumb
    if project_name.is_some() || session_name.is_some() {
        let mut crumb = String::new();
        if let Some(p) = project_name {
            crumb.push_str(p);
        }
        if project_name.is_some() && session_name.is_some() {
            crumb.push_str(" / ");
        }
        if let Some(s) = session_name {
            crumb.push_str(s);
        }
        items.push(
            text(crumb)
                .size(12)
                .color(theme.text_secondary())
                .font(Font::MONOSPACE)
                .into(),
        );

        // Divider
        items.push(Space::new().width(12.0).into());
        items.push(
            container(Space::new().width(1.0).height(16.0))
                .style(|_: &Theme| container::Style {
                    background: Some(theme.border().into()),
                    ..Default::default()
                })
                .into(),
        );
        items.push(Space::new().width(12.0).into());
    }

    // Flex space (future tabs go here)
    items.push(Space::new().width(Length::Fill).into());

    // Right padding
    items.push(Space::new().width(12.0).into());

    let bar = container(
        row(items)
            .align_y(iced::Alignment::Center)
            .height(28.0)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(theme.panel_bg().into()),
        ..Default::default()
    });

    mouse_area(bar).on_press(on_drag).into()
}
