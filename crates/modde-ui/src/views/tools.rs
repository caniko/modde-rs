use crate::views::selectable_text::{TextStyle, danger, secondary, success, text, warning};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text_input, toggler,
};
use iced::{Alignment, Element, Length};

use modde_games::tools::ToolSettingKind;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{ChecklistStatus, Message, ToolHistoryUiEntry, ToolState, ToolUiEntry};
use crate::semantics;
use crate::views::tabs::{Tab, tab_bar};

/// Render the gaming tools/overlays management view.
pub fn view(state: &ToolState) -> Element<'_, Message> {
    let title = state.game_label.as_deref().map_or_else(
        || "Gaming Tools".to_string(),
        |game| format!("Gaming Tools - {game}"),
    );
    let title_bar = row![
        text(title).size(20),
        iced::widget::space::horizontal(),
        semantics::test_id(
            "tools.refresh",
            button(text("Refresh").size(14))
                .style(button::secondary)
                .padding([6, 14])
                .on_action_maybe(
                    (!state.loading).then_some(ButtonAction::RefreshTools),
                    "Tools are already loading.",
                ),
        ),
    ]
    .align_y(Alignment::Center);

    if state.entries.is_empty() {
        let message = if state.loading {
            "Loading tools..."
        } else {
            "Select a game to manage tools, or click Refresh."
        };
        return column![
            title_bar,
            container(text(message).size(14))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill),
        ]
        .spacing(12)
        .padding(12)
        .width(Length::Fill)
        .into();
    }

    let active_tool_id = state
        .active_tool_id
        .as_deref()
        .unwrap_or_else(|| state.entries[0].tool_id.as_str());
    let active_entry = state
        .entries
        .iter()
        .find(|entry| entry.tool_id == active_tool_id)
        .unwrap_or(&state.entries[0]);

    let tabs = tab_bar(state.entries.iter().map(|entry| {
        Tab::new(
            entry.display_name.clone(),
            entry.tool_id == active_entry.tool_id,
            ButtonAction::SelectToolTab(entry.tool_id.clone()),
        )
        .test_id(format!("tools.tab.{}", entry.tool_id))
    }));

    let mut content = column![title_bar, tabs, iced::widget::rule::horizontal(1)].spacing(10);
    if state.loading {
        content = content.push(text("Loading tools...").size(12).style(secondary));
    }
    if let Some(error) = &state.load_error {
        content = content.push(
            text(format!("Failed to load tools: {error}"))
                .size(12)
                .style(danger),
        );
    }
    let panel = tool_panel(active_entry, state);

    content
        .push(panel)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tool_panel<'a>(entry: &'a ToolUiEntry, state: &'a ToolState) -> Element<'a, Message> {
    let available_color: TextStyle = if entry.available { success } else { danger };

    let toggle = toggler(entry.enabled)
        .on_toggle_maybe((entry.available && !state.loading).then_some({
            let tool_id = entry.tool_id.clone();
            move |enabled| Message::ToggleTool {
                tool_id: tool_id.clone(),
                enabled,
            }
        }))
        .size(18.0);

    let header = row![
        column![
            text(entry.display_name.as_str()).size(18),
            text(entry.category.as_str()).size(12).style(secondary),
        ]
        .spacing(2),
        iced::widget::space::horizontal(),
        text(entry.availability_text.as_str())
            .size(12)
            .style(available_color),
        row![text("Enabled").size(12).style(secondary), toggle,]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center)
    .spacing(16);

    let mut body = column![
        header,
        text(entry.description.as_str()).size(13),
        tool_specific_actions(entry, state),
        config_checklist_panel(entry),
        settings_panel(entry, state.show_advanced_settings),
        history_panel(entry),
        derived_facts_panel(entry),
        preview_panel(entry),
    ]
    .spacing(12);

    if entry.tool_id == "optiscaler" {
        body = body.push(optiscaler_state_actions(entry, state.game_dir_configured));
    }

    if let Some(ref msg) = entry.status_message {
        body = body.push(text(msg.as_str()).size(12).style(success));
    }

    let scrollable_panel = scrollable(
        container(body)
            .padding(12)
            .width(Length::Fill)
            .style(container::rounded_box),
    )
    .id(semantics::widget_id(format!(
        "tools.{}.scroll",
        entry.tool_id
    )))
    .height(Length::Fill);

    if entry.has_file_patching {
        column![
            scrollable_panel,
            bottom_action_bar(
                entry,
                state.game_dir_configured,
                state.is_tool_busy(&entry.tool_id),
                state.loading,
            ),
        ]
        .spacing(8)
        .height(Length::Fill)
        .into()
    } else {
        scrollable_panel.into()
    }
}

fn has_ini_overrides(entry: &ToolUiEntry) -> bool {
    entry
        .settings
        .get("ini_overrides")
        .and_then(|v| v.as_object())
        .is_some_and(|m| !m.is_empty())
}

fn optiscaler_state_actions(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
) -> Element<'_, Message> {
    let can_adopt = game_dir_configured && entry.optiscaler_detected_files > 0;
    let can_restore = game_dir_configured && entry.optiscaler_latest_backup.is_some();
    let state = entry
        .optiscaler_state
        .as_deref()
        .unwrap_or("no OptiScaler install detected");
    row![
        text(state).size(12).style(secondary),
        iced::widget::space::horizontal(),
        semantics::test_id(
            "tools.optiscaler.adopt",
            button(text("Adopt").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action_maybe(
                    can_adopt.then_some(ButtonAction::AdoptOptiScaler),
                    "No detected OptiScaler files are available to adopt.",
                ),
        ),
        semantics::test_id(
            "tools.optiscaler.restore_backup",
            button(text("Restore backup").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action_maybe(
                    can_restore.then_some(ButtonAction::RestoreOptiScalerBackup),
                    "No OptiScaler backup exists for this game.",
                ),
        ),
        semantics::test_id(
            "tools.optiscaler.reset_config",
            button(text("Reset config").size(12))
                .style(button::danger)
                .padding([4, 10])
                .on_action_maybe(
                    has_ini_overrides(entry).then_some(ButtonAction::ResetOptiScalerConfig),
                    "No INI overrides to reset.",
                ),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn tool_specific_actions<'a>(entry: &'a ToolUiEntry, state: &'a ToolState) -> Element<'a, Message> {
    if entry.tool_id == "optiscaler" && entry.release_support.is_supported() {
        return optiscaler_release_panel(entry, state);
    }

    match entry.tool_id.as_str() {
        "proton" => {
            let versions = state
                .tool_option_catalog
                .get("proton.selected_version")
                .map_or(0, Vec::len);
            row![
                semantics::test_id(
                    "tools.proton.versions.refresh",
                    button(
                        text(if state.proton_versions_loading {
                            "Loading versions"
                        } else {
                            "Refresh versions"
                        })
                        .size(12)
                    )
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action_maybe(
                        (!state.loading && !state.proton_versions_loading)
                            .then_some(ButtonAction::RefreshProtonVersions),
                        "Proton versions are already loading.",
                    ),
                ),
                semantics::test_id(
                    "tools.proton.install_selected",
                    button(text("Install with protonup-rs").size(12))
                        .style(button::primary)
                        .padding([4, 10])
                        .on_action_maybe(
                            (!state.loading && versions > 0)
                                .then_some(ButtonAction::InstallProtonVersion),
                            "No Proton versions are available from protonup-rs.",
                        ),
                ),
                text(format!("{versions} version option(s)")).size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        _ => iced::widget::space::vertical()
            .height(Length::Shrink)
            .into(),
    }
}

fn optiscaler_release_panel<'a>(
    entry: &'a ToolUiEntry,
    state: &'a ToolState,
) -> Element<'a, Message> {
    let source_mode = setting_value_as_string(setting_value(&entry.settings, "source_mode"));
    let mut rows = column![text("OptiScaler Release").size(14)].spacing(10);

    if let Some(spec) = tool_setting_spec(entry, "source_mode") {
        rows = rows.push(setting_row(entry, spec));
    }
    if source_mode == "goverlay_builds"
        && let Some(spec) = tool_setting_spec(entry, "goverlay_channel")
    {
        rows = rows.push(setting_row(entry, spec));
    }
    if source_mode == "local_dir" {
        if let Some(spec) = tool_setting_spec(entry, "local_source_dir") {
            rows = rows.push(setting_row(entry, spec));
        }
        return container(rows)
            .padding(10)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into();
    }
    if let Some(spec) = tool_setting_spec(entry, "release_tag") {
        rows = rows.push(setting_row(entry, spec));
    }
    if let Some(spec) = tool_setting_spec(entry, "release_asset") {
        rows = rows.push(optiscaler_release_asset_row(entry, spec, state));
    } else {
        rows = rows.push(optiscaler_release_action_row(state));
    }

    container(rows)
        .padding(10)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn optiscaler_release_asset_row<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
    state: &'a ToolState,
) -> Element<'a, Message> {
    row![
        column![
            text(&*spec.label).size(13),
            text(&*spec.description).size(11).style(secondary),
        ]
        .spacing(2)
        .width(Length::FillPortion(1)),
        row![
            container(setting_control(entry, spec)).width(Length::Fill),
            optiscaler_release_action_row(state),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::FillPortion(2)),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn optiscaler_release_action_row(state: &ToolState) -> Element<'_, Message> {
    let can_refresh = !state.loading && !state.optiscaler_releases_loading;
    let can_install = !state.loading
        && !state.optiscaler_releases_loading
        && state
            .tool_option_catalog
            .get("optiscaler.release_asset")
            .is_some_and(|options| !options.is_empty());
    row![
        semantics::test_id(
            "tools.optiscaler.releases.refresh",
            button(
                text(if state.optiscaler_releases_loading {
                    "Loading releases"
                } else {
                    "Refresh releases"
                })
                .size(12)
            )
            .style(button::secondary)
            .padding([4, 10])
            .on_action_maybe(
                can_refresh.then_some(ButtonAction::RefreshOptiScalerReleases),
                "OptiScaler releases are already loading.",
            ),
        ),
        semantics::test_id(
            "tools.optiscaler.releases.install_selected",
            button(text("Install selected release").size(12))
                .style(button::primary)
                .padding([4, 10])
                .on_action_maybe(
                    can_install.then_some(ButtonAction::InstallOptiScalerRelease),
                    "Load releases and select an installable asset before installing.",
                ),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn config_checklist_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    if entry.config_checklist.is_empty() {
        return iced::widget::space::vertical()
            .height(Length::Shrink)
            .into();
    }
    let all_done = entry
        .config_checklist
        .iter()
        .all(|item| item.status == ChecklistStatus::Done);
    let header = row![
        text("Setup Progress").size(14),
        iced::widget::space::horizontal(),
        text(if all_done { "All steps complete" } else { "" })
            .size(12)
            .style(success),
    ]
    .align_y(Alignment::Center);
    let mut rows = column![header].spacing(6);
    for item in &entry.config_checklist {
        let icon = match item.status {
            ChecklistStatus::Done => text("✓").size(13).style(success),
            ChecklistStatus::Pending => text("○").size(13).style(warning),
            ChecklistStatus::Blocked => text("✗").size(13).style(danger),
        };
        let mut row_content = row![icon, text(item.label.as_str()).size(13)].spacing(8);
        if let Some(hint) = &item.hint {
            row_content = row_content.push(text(format!("— {hint}")).size(12).style(secondary));
        }
        rows = rows.push(row_content.align_y(Alignment::Center));
    }
    container(rows)
        .padding(10)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

#[path = "tools_parts/panels.rs"]
mod panels;
#[path = "tools_parts/settings.rs"]
mod settings;

use self::panels::{bottom_action_bar, history_panel, preview_panel};
use self::settings::{
    derived_facts_panel, setting_control, setting_row, setting_value, setting_value_as_string,
    settings_panel, tool_setting_spec,
};
