use crate::views::selectable_text::{danger, secondary, text};
use iced::widget::{button, column, container, row, text_input};
use iced::{Alignment, Element, Length};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{ExecutableDraftField, Message, ToolState};

/// Render the named executable launch target manager.
pub fn view(state: &ToolState) -> Element<'_, Message> {
    let title = state.game_label.as_deref().map_or_else(
        || "Executables".to_string(),
        |game| format!("Executables - {game}"),
    );
    let title_bar = row![
        text(title).size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action_maybe(
                (!state.executables_loading).then_some(ButtonAction::RefreshExecutables),
                "Executables are already loading.",
            ),
    ]
    .align_y(Alignment::Center);

    let mut content = column![title_bar].spacing(10);
    if state.executables_loading {
        content = content.push(text("Loading executables...").size(12).style(secondary));
    }
    if let Some(error) = &state.executables_load_error {
        content = content.push(
            text(format!("Failed to load executables: {error}"))
                .size(12)
                .style(danger),
        );
    }

    content
        .push(executables_panel(state))
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn executables_panel(state: &ToolState) -> Element<'_, Message> {
    let mut rows = column![].spacing(6);
    if state.executables.is_empty() {
        rows = rows.push(
            container(text("No executables configured").size(12).style(secondary)).padding([4, 0]),
        );
    } else {
        for entry in &state.executables {
            let busy = state.is_executable_busy(&entry.name);
            let metadata = executable_metadata(entry);
            rows = rows.push(
                container(
                    column![
                        row![
                            column![
                                text(entry.name.as_str()).size(14),
                                text(entry.executable_path.as_str())
                                    .size(11)
                                    .style(secondary),
                                text(metadata).size(11).style(secondary),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            button(text(if busy { "Running" } else { "Run" }).size(12))
                                .style(button::success)
                                .padding([4, 10])
                                .on_action_maybe(
                                    (!busy)
                                        .then_some(ButtonAction::RunExecutable(entry.name.clone())),
                                    "This executable is already running.",
                                ),
                            button(text("Edit").size(12))
                                .style(button::secondary)
                                .padding([4, 10])
                                .on_action(ButtonAction::EditExecutable(entry.name.clone())),
                            button(text("Remove").size(12))
                                .style(button::danger)
                                .padding([4, 10])
                                .on_action_maybe(
                                    (!busy).then_some(ButtonAction::RemoveExecutable(
                                        entry.name.clone()
                                    )),
                                    "This executable is already running.",
                                ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(4),
                )
                .padding(8)
                .width(Length::Fill)
                .style(container::rounded_box),
            );
        }
    }

    let count_label = match state.executables.len() {
        0 => "0 configured".to_string(),
        1 => "1 configured".to_string(),
        count => format!("{count} configured"),
    };
    let editor_visible = state.executables.is_empty() || state.executable_editor_open;

    let mut panel = column![
        row![
            text("Executables").size(16),
            text(count_label).size(11).style(secondary),
            iced::widget::space::horizontal(),
            button(text("Add executable").size(12))
                .style(button::secondary)
                .padding([5, 12])
                .on_action(ButtonAction::OpenExecutableEditor),
        ]
        .align_y(Alignment::Center),
        rows,
    ]
    .spacing(8);

    if let Some(error) = &state.executable_error {
        panel = panel.push(text(error.as_str()).size(12).style(danger));
    }
    if editor_visible {
        panel = panel.push(executable_form(state));
    }

    container(panel)
        .padding(12)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn executable_form(state: &ToolState) -> Element<'_, Message> {
    let draft = &state.executable_draft;
    let save_label = if draft.name.trim().is_empty() {
        "Save"
    } else {
        "Save / update"
    };

    column![
        row![
            text_input("Name", &draft.name)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::Name,
                    value,
                })
                .width(Length::FillPortion(2)),
            text_input("Executable path", &draft.executable_path)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::Path,
                    value,
                })
                .width(Length::FillPortion(4)),
            button(text("Browse").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::BrowseExecutablePath),
        ]
        .spacing(8),
        row![
            text_input("Arguments", &draft.arguments)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::Arguments,
                    value,
                })
                .width(Length::Fill),
            text_input("Output mod", &draft.output_mod)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::OutputMod,
                    value,
                })
                .width(Length::FillPortion(1)),
        ]
        .spacing(8),
        row![
            text_input("Working directory", &draft.working_dir)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::WorkingDir,
                    value,
                })
                .width(Length::Fill),
            button(text("Browse").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::BrowseExecutableWorkingDir),
        ]
        .spacing(8),
        row![
            text_input("WINEDLLOVERRIDES", &draft.wine_dll_overrides)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::WineDllOverrides,
                    value,
                })
                .width(Length::Fill),
            text_input("Env lines KEY=VALUE", &draft.environment)
                .on_input(|value| Message::UpdateExecutableDraft {
                    field: ExecutableDraftField::Environment,
                    value,
                })
                .width(Length::Fill),
        ]
        .spacing(8),
        row![
            button(text(save_label).size(12))
                .style(button::primary)
                .padding([5, 12])
                .on_action(ButtonAction::SaveExecutable),
            button(text("Clear").size(12))
                .style(button::secondary)
                .padding([5, 12])
                .on_action(ButtonAction::ClearExecutableDraft),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .into()
}

fn executable_metadata(entry: &crate::app::ExecutableUiEntry) -> String {
    let mut parts = Vec::new();
    if !entry.arguments.is_empty() {
        parts.push(format!("args: {}", entry.arguments));
    }
    if !entry.working_dir.is_empty() {
        parts.push(format!("cwd: {}", entry.working_dir));
    }
    if !entry.wine_dll_overrides.is_empty() {
        parts.push(format!("dll: {}", entry.wine_dll_overrides));
    }
    parts.push(format!("output: {}", entry.output_mod));
    parts.join(" | ")
}
