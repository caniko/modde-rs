use std::path::PathBuf;

use crate::views::selectable_text::text;
use iced::widget::{button, column, row, scrollable};
use iced::{Element, Length};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

/// State for the overwrite management view.
#[derive(Debug, Clone, Default)]
pub struct OverwriteState {
    pub files: Vec<String>, // relative paths in the overrides directory
    pub overrides_dir: PathBuf,
}

pub fn view(state: &OverwriteState) -> Element<'_, Message> {
    let title = text("Overwrite / Profile Overrides").size(20);

    let count_text = text(format!("{} file(s) in overrides", state.files.len())).size(14);

    if state.files.is_empty() {
        return column![
            title,
            count_text,
            text("No override files. Files placed here will win over all mods during deploy.")
                .size(12),
        ]
        .spacing(12)
        .padding(16)
        .into();
    }

    let file_list: Vec<Element<Message>> = state
        .files
        .iter()
        .map(|f| {
            row![text(f).size(12).width(Length::Fill),]
                .padding(4)
                .into()
        })
        .collect();

    let actions = row![
        button(text("Clear All").size(12))
            .style(button::danger)
            .padding([4, 12])
            .on_action(ButtonAction::ClearOverwrite.confirmed(
                "Clear overrides",
                "Delete every file in the overrides directory? This cannot be undone.",
            )),
        button(text("Create Mod from Overrides").size(12))
            .padding([4, 12])
            .on_action(ButtonAction::MoveOverwriteToMod(
                "__from_overrides__".to_string(),
            )),
    ]
    .spacing(8);

    column![
        title,
        count_text,
        actions,
        scrollable(column(file_list).spacing(2)).height(Length::Fill),
    ]
    .spacing(12)
    .padding(16)
    .into()
}
