#![allow(clippy::wildcard_imports)]
use crate::views::selectable_text::{danger, secondary, text};
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input};
use iced::{Alignment, Element, Length};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{AddCustomGameDraftField, AddCustomGameState, Message};
use crate::semantics;

pub fn add_dialog(state: &AddCustomGameState) -> Element<'_, Message> {
    let draft = &state.draft;
    let selected_dir = draft
        .executable_dir
        .as_ref()
        .and_then(|value| {
            state
                .detected_dirs
                .iter()
                .find(|dir| dir.relative_dir == *value)
        })
        .cloned();

    let mut body = column![
        text("Add Custom Game").size(18),
        row![
            text_input("Game id", &draft.id)
                .id(semantics::widget_id("add_custom_game.input.id"))
                .on_input(|value| Message::AddCustomGameFieldChanged {
                    field: AddCustomGameDraftField::Id,
                    value,
                })
                .padding(8)
                .width(Length::Fill),
            text_input("Display name", &draft.display_name)
                .id(semantics::widget_id("add_custom_game.input.display_name"))
                .on_input(|value| Message::AddCustomGameFieldChanged {
                    field: AddCustomGameDraftField::DisplayName,
                    value,
                })
                .padding(8)
                .width(Length::Fill),
        ]
        .spacing(8),
        row![
            text_input("Install path", &draft.install_path)
                .id(semantics::widget_id("add_custom_game.input.install_path"))
                .on_input(|value| Message::AddCustomGameFieldChanged {
                    field: AddCustomGameDraftField::InstallPath,
                    value,
                })
                .padding(8)
                .width(Length::Fill),
            semantics::test_id(
                "add_custom_game.install_path.browse",
                button(text("Browse").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::BrowseAddCustomGameInstallPath),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        semantics::test_id(
            "add_custom_game.input.executable_dir",
            pick_list(state.detected_dirs.clone(), selected_dir, |dir| {
                Message::AddCustomGameFieldChanged {
                    field: AddCustomGameDraftField::ExecutableDir,
                    value: dir.relative_dir,
                }
            })
            .placeholder("Executable directory")
            .width(Length::Fill),
        ),
        row![
            text_input(
                "Steam app id (optional)",
                draft.steam_app_id.as_deref().unwrap_or(""),
            )
            .id(semantics::widget_id("add_custom_game.input.steam_app_id"))
            .on_input(|value| Message::AddCustomGameFieldChanged {
                field: AddCustomGameDraftField::SteamAppId,
                value,
            })
            .padding(8)
            .width(Length::Fill),
            text_input(
                "Nexus domain (optional)",
                draft.nexus_domain.as_deref().unwrap_or(""),
            )
            .id(semantics::widget_id("add_custom_game.input.nexus_domain"))
            .on_input(|value| Message::AddCustomGameFieldChanged {
                field: AddCustomGameDraftField::NexusDomain,
                value,
            })
            .padding(8)
            .width(Length::Fill),
        ]
        .spacing(8),
        text_input(
            "Proxy DLLs, comma-separated (optional)",
            &draft.proxy_dlls_csv,
        )
        .id(semantics::widget_id("add_custom_game.input.proxy_dlls"))
        .on_input(|value| Message::AddCustomGameFieldChanged {
            field: AddCustomGameDraftField::ProxyDlls,
            value,
        })
        .padding(8)
        .width(Length::Fill),
    ]
    .spacing(10);

    if !state.detected_dirs.is_empty() {
        let detect_rows = state.detected_dirs.iter().enumerate().fold(
            column![text("Detected executable directories").size(12)],
            |column, (index, dir)| {
                column.push(semantics::test_id(
                    format!("add_custom_game.detect.{index}"),
                    text(format!(
                        "{} ({})",
                        dir.relative_dir,
                        dir.exe_names.join(", ")
                    ))
                    .size(11),
                ))
            },
        );
        body = body.push(detect_rows.spacing(4));
    }

    if let Some(error) = &state.error {
        body = body.push(text(error).size(12).style(danger));
    }

    body = body.push(
        row![
            iced::widget::Space::new().width(Length::Fill),
            semantics::test_id(
                "add_custom_game.cancel",
                button(text("Cancel").size(13))
                    .style(button::secondary)
                    .padding([6, 14])
                    .on_action(ButtonAction::AddCustomGameCancel),
            ),
            semantics::test_id(
                "add_custom_game.submit",
                button(text("Save").size(13))
                    .style(button::success)
                    .padding([6, 14])
                    .on_action_maybe(
                        state
                            .can_submit()
                            .then_some(ButtonAction::AddCustomGameSubmit),
                        "Enter a valid id, display name, install path, and executable directory.",
                    ),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    container(body)
        .width(Length::Fixed(720.0))
        .padding(16)
        .style(container::rounded_box)
        .into()
}

pub fn manage_dialog(custom_games: Vec<(String, String)>) -> Element<'static, Message> {
    let mut rows = column![text("Manage Custom Games").size(18)].spacing(10);
    if custom_games.is_empty() {
        rows = rows.push(
            text("No custom games are registered.")
                .size(12)
                .style(secondary),
        );
    } else {
        let list = custom_games
            .into_iter()
            .fold(column![].spacing(8), |column, (id, label)| {
                column.push(
                    container(
                        row![
                            column![
                                text(label).size(14),
                                text(id.clone()).size(11).style(secondary)
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            semantics::test_id(
                                format!("manage_custom_games.remove.{id}"),
                                button(text("Remove").size(12))
                                    .style(button::danger)
                                    .padding([5, 12])
                                    .on_action(ButtonAction::RemoveCustomGame(id.clone())),
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .padding(8)
                    .style(container::rounded_box),
                )
            });
        rows = rows.push(scrollable(list).height(Length::Fixed(280.0)));
    }

    rows = rows.push(
        row![
            iced::widget::Space::new().width(Length::Fill),
            semantics::test_id(
                "manage_custom_games.close",
                button(text("Close").size(13))
                    .style(button::secondary)
                    .padding([6, 14])
                    .on_action(ButtonAction::CloseManageCustomGames),
            ),
        ]
        .align_y(Alignment::Center),
    );

    container(rows)
        .width(Length::Fixed(520.0))
        .padding(16)
        .style(container::rounded_box)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AddCustomGameDraft;

    #[test]
    fn add_custom_game_view_renders() {
        let state = AddCustomGameState {
            draft: AddCustomGameDraft {
                id: "elden-ring".to_string(),
                display_name: "ELDEN RING".to_string(),
                install_path: "/games/elden-ring".to_string(),
                executable_dir: Some("Game".to_string()),
                steam_app_id: Some("1245620".to_string()),
                nexus_domain: Some("eldenring".to_string()),
                proxy_dlls_csv: "dxgi,winmm".to_string(),
            },
            detected_dirs: vec![modde_games::DetectCandidateDir {
                relative_dir: "Game".to_string(),
                exe_names: vec!["eldenring.exe".to_string()],
                total_size: 1,
            }],
            error: None,
        };

        let element = add_dialog(&state);
        let _ = element;
    }
}
