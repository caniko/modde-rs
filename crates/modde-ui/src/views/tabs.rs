use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::semantics;
use crate::views::selectable_text::{primary, text};
use iced::widget::{button, column, row};
use iced::{Alignment, Element, Length};

pub struct Tab {
    pub label: String,
    pub active: bool,
    pub action: ButtonAction,
    pub test_id: Option<String>,
}

impl Tab {
    pub fn new(label: impl Into<String>, active: bool, action: ButtonAction) -> Self {
        Self {
            label: label.into(),
            active,
            action,
            test_id: None,
        }
    }

    #[must_use]
    pub fn test_id(mut self, test_id: impl Into<String>) -> Self {
        self.test_id = Some(test_id.into());
        self
    }
}

pub fn tab_bar(tabs: impl IntoIterator<Item = Tab>) -> Element<'static, crate::app::Message> {
    tabs.into_iter()
        .fold(row![].spacing(6), |bar, tab| {
            let label = if tab.active {
                text(tab.label.clone()).size(13).style(primary)
            } else {
                text(tab.label.clone()).size(13)
            };
            let btn = button(label).padding([4, 10]).style(button::text);
            let tab_button = if tab.active {
                btn.described_disabled("This tab is already open.")
            } else {
                btn.on_action(tab.action)
            };
            let tab_button = if let Some(test_id) = tab.test_id {
                semantics::test_id(test_id, tab_button)
            } else {
                tab_button
            };
            let indicator = if tab.active {
                text("━━━━").size(10).style(primary)
            } else {
                text("").size(10)
            };
            bar.push(
                column![tab_button, indicator]
                    .spacing(0)
                    .align_x(Alignment::Center)
                    .width(Length::Shrink),
            )
        })
        .align_y(Alignment::Center)
        .into()
}
