use crate::views::selectable_text::text;
use iced::widget::{column, container, row, scrollable, text_input};
use iced::{Alignment, Element, Length};

use crate::app::Message;

/// State for the data tab view.
#[derive(Debug, Clone, Default)]
pub struct DataTabState {
    pub filter: String,
    pub missing_store_mod_count: usize,
}

/// Render the data tab view showing file-level conflict details.
pub fn view<'a>(
    state: &'a DataTabState,
    conflict_files: &'a [(String, Vec<String>)],
) -> Element<'a, Message> {
    let title_bar = row![
        text("Data Files").size(20),
        iced::widget::space::horizontal(),
    ]
    .align_y(Alignment::Center);

    let toolbar = row![
        text_input("Filter files...", &state.filter)
            .on_input(Message::DataTabFilterChanged)
            .padding(6)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let filter_lower = state.filter.to_lowercase();
    let filtered: Vec<&(String, Vec<String>)> = conflict_files
        .iter()
        .filter(|(file, _)| filter_lower.is_empty() || file.to_lowercase().contains(&filter_lower))
        .collect();

    let file_count = filtered.len();

    let file_rows: Element<Message> = if filtered.is_empty() && state.missing_store_mod_count > 0 {
        container(
            text(format!(
                "{} enabled mod(s) are missing from the store; conflict data is incomplete.",
                state.missing_store_mod_count
            ))
            .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else if filtered.is_empty() {
        container(text("No data files to display.").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    } else {
        let rows = filtered
            .into_iter()
            .fold(column![].spacing(4), |col, (file, providers)| {
                let provider_list = providers.join(", ");
                let file_row = row![
                    text(file).size(13).width(Length::Fill),
                    text(provider_list).size(12).width(Length::Fixed(300.0)),
                ]
                .spacing(8)
                .padding([4, 8]);
                col.push(file_row)
            });

        scrollable(rows).height(Length::Fill).into()
    };

    let header = row![
        text("File Path").size(12).width(Length::Fill),
        text("Providing Mods").size(12).width(Length::Fixed(300.0)),
    ]
    .spacing(8)
    .padding([4, 8]);

    let status = text(format!("{file_count} file(s) shown")).size(12);

    column![
        title_bar,
        toolbar,
        header,
        iced::widget::rule::horizontal(1),
        file_rows,
        status,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
