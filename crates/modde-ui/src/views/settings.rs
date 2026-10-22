use std::path::PathBuf;

use crate::views::selectable_text::{danger, secondary, success, text, warning};
use iced::widget::{button, column, pick_list, row, scrollable, text_input};
use iced::{Alignment, Element, Length};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, NexusAuthStatus, SettingsState};
use crate::semantics;

/// Render the settings view.
pub fn view(state: SettingsState) -> Element<'static, Message> {
    let title = text("Settings").size(20);

    // Nexus API key with validation status
    let nexus_status_widget: Element<'static, Message> = match &state.nexus_status {
        Some(NexusAuthStatus::Checking) => text("Checking...").size(12).style(warning).into(),
        Some(NexusAuthStatus::Valid {
            username,
            is_premium,
        }) => {
            let tier = if *is_premium { "Premium" } else { "Standard" };
            text(format!("Logged in as {username} ({tier})"))
                .size(12)
                .style(success)
                .into()
        }
        Some(NexusAuthStatus::Invalid(err)) => text(format!("Invalid: {err}"))
            .size(12)
            .style(danger)
            .into(),
        None => text("Not validated").size(12).into(),
    };

    let nexus_source = state
        .nexus_api_key_source
        .as_ref()
        .map(|source| format!("Using {}", source.label()))
        .unwrap_or_else(|| "No key configured".to_string());
    let show_hide_label = if state.nexus_api_key_visible {
        "Hide"
    } else {
        "Show"
    };

    let api_key_section = column![
        text("Nexus Mods API Key").size(14),
        text("Required for downloading mods and browsing collections.").size(11),
        row![
            text_input("Enter your API key...", &state.nexus_api_key_draft)
                .id(semantics::widget_id("settings.nexus_api_key"))
                .secure(!state.nexus_api_key_visible)
                .on_input(Message::SetNexusApiKeyDraft)
                .padding(8)
                .width(Length::Fill),
            semantics::test_id(
                "settings.nexus_api_key.toggle_visibility",
                button(text(show_hide_label).size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::ToggleNexusApiKeyVisibility),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            semantics::test_id(
                "settings.nexus_api_key.replace",
                button(text("Replace").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::ReplaceNexusApiKey),
            ),
            semantics::test_id(
                "settings.nexus_api_key.remove_modde_config",
                button(text("Remove modde config").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::RemoveNexusConfigKey.confirmed(
                        "Remove Nexus API key",
                        "Remove the API key saved in modde's config file?",
                    )),
            ),
            semantics::test_id(
                "settings.nexus_api_key.validate",
                button(text("Validate").size(13))
                    .style(button::primary)
                    .padding([6, 12])
                    .on_action(ButtonAction::ValidateNexusKey),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(nexus_source).size(12),
        text(if state.nexus_config_key_exists {
            "modde config key exists"
        } else {
            "No modde config key"
        })
        .size(11),
        nexus_status_widget,
    ]
    .spacing(4);

    // Game install path
    let game_path_section = column![
        text("Game Install Path").size(14),
        text("Root directory of the game installation. Press Enter to apply.").size(11),
        row![
            text_input("/path/to/game", &state.game_path_draft)
                .id(semantics::widget_id("settings.game_path"))
                .on_input(Message::GamePathDraftChanged)
                .on_submit_maybe(state.current_game_id.clone().map(|game_id| {
                    Message::SetGamePath {
                        game_id,
                        path: PathBuf::from(&state.game_path_draft),
                    }
                }))
                .padding(8)
                .width(Length::Fill),
            semantics::test_id(
                "settings.game_path.browse",
                button(text("Browse").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::BrowseGamePath),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Download directory
    let download_dir_section = column![
        text("Download Directory").size(14),
        text("Where downloaded mod archives are stored. Press Enter to apply.").size(11),
        row![
            text_input("/path/to/downloads", &state.download_dir_draft)
                .id(semantics::widget_id("settings.download_dir"))
                .on_input(Message::DownloadDirDraftChanged)
                .on_submit(Message::SetDownloadDir(PathBuf::from(
                    &state.download_dir_draft
                )))
                .padding(8)
                .width(Length::Fill),
            semantics::test_id(
                "settings.download_dir.browse",
                button(text("Browse").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::BrowseDownloadDir),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Stock game snapshot with verify button
    let stock_game_section = column![
        text("Stock Game Snapshot").size(14),
        text("Create a snapshot of your clean game install for virtual deployment.").size(11),
        row![
            semantics::test_id(
                "settings.stock_snapshot.create",
                button(text("Create Snapshot").size(13))
                    .style(button::primary)
                    .padding([6, 14])
                    .on_action(ButtonAction::CreateStockSnapshot),
            ),
            semantics::test_id(
                "settings.stock_snapshot.verify",
                button(text("Verify Snapshot").size(13))
                    .style(button::secondary)
                    .padding([6, 14])
                    .on_action(ButtonAction::VerifyStockSnapshot),
            ),
            text(if state.has_stock_snapshot {
                "Snapshot exists"
            } else {
                "No snapshot created"
            })
            .size(12),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Theme selection
    let theme_options: Vec<String> = crate::app::THEMES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let theme_section = column![
        text("Theme").size(14),
        pick_list(
            theme_options,
            Some(state.theme_name.clone()),
            Message::SetTheme,
        )
        .width(Length::Fixed(200.0)),
    ]
    .spacing(4);

    let shortcuts_section = column![
        text("Keyboard shortcuts").size(14),
        text(crate::shortcuts::help_text())
            .size(11)
            .style(secondary),
    ]
    .spacing(4);

    let content = scrollable(
        column![
            api_key_section,
            iced::widget::rule::horizontal(1),
            game_path_section,
            iced::widget::rule::horizontal(1),
            download_dir_section,
            iced::widget::rule::horizontal(1),
            stock_game_section,
            iced::widget::rule::horizontal(1),
            theme_section,
            iced::widget::rule::horizontal(1),
            shortcuts_section,
        ]
        .spacing(16)
        .padding(16),
    )
    .height(Length::Fill);

    column![title, iced::widget::rule::horizontal(1), content,]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
