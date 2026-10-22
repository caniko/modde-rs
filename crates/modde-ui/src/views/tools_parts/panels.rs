#![allow(clippy::wildcard_imports)]
use super::settings::{setting_value, setting_value_as_string};
use super::*;
use iced::widget::column;

pub(super) fn preview_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    let mut preview = column![text("Launch Integration").size(14)].spacing(6);

    if let Some(path) = &entry.generated_config_path {
        preview = preview.push(text(format!("Config: {path}")).size(12));
    }
    if !entry.env_preview.is_empty() {
        preview = preview.push(
            text(format!(
                "Env: {}",
                entry
                    .env_preview
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .size(12),
        );
    }
    if !entry.dll_overrides.is_empty() {
        preview = preview
            .push(text(format!("DLL overrides: {}", entry.dll_overrides.join(", "))).size(12));
    }
    if !entry.wrapper_preview.is_empty() {
        preview =
            preview.push(text(format!("Wrappers: {}", entry.wrapper_preview.join(", "))).size(12));
    }
    if entry.generated_config_path.is_none()
        && entry.env_preview.is_empty()
        && entry.dll_overrides.is_empty()
        && entry.wrapper_preview.is_empty()
    {
        preview =
            preview.push(text("No launch integration preview for current settings.").size(12));
    }

    preview.into()
}

pub(super) fn history_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    let mut rows = column![text("Settings History").size(14)].spacing(6);
    if entry.setting_history.is_empty() {
        return rows
            .push(
                text("No settings history recorded yet.")
                    .size(12)
                    .style(secondary),
            )
            .into();
    }

    for node in &entry.setting_history {
        rows = rows.push(history_row(entry, node));
    }
    rows.into()
}

fn history_row<'a>(entry: &'a ToolUiEntry, node: &'a ToolHistoryUiEntry) -> Element<'a, Message> {
    let marker = if node.is_current {
        "current"
    } else {
        "version"
    };
    let state = if node.enabled { "enabled" } else { "disabled" };
    row![
        column![
            text(format!("{marker}: {}", node.label)).size(12),
            text(format!("{} - {state}", node.human_reason))
                .size(11)
                .style(secondary),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(text("Restore").size(12))
            .style(button::secondary)
            .padding([4, 10])
            .on_action_maybe(
                (!node.is_current).then_some(ButtonAction::RestoreToolSettings {
                    tool_id: entry.tool_id.clone(),
                    node_id: node.node_id.clone(),
                }),
                "This settings version is already current.",
            ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn bottom_action_bar(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> Element<'_, Message> {
    let (can_apply, apply_disabled_reason) =
        apply_readiness(entry, game_dir_configured, tool_busy, tools_loading);
    let can_revert =
        game_dir_configured && !entry.applied_files.is_empty() && !tool_busy && !tools_loading;
    let revert_readiness = if tools_loading {
        RevertReadiness::ToolsLoading
    } else if tool_busy {
        RevertReadiness::ToolBusy
    } else if !game_dir_configured {
        RevertReadiness::MissingGameDir
    } else if entry.applied_files.is_empty() {
        RevertReadiness::NoAppliedFiles
    } else {
        RevertReadiness::Ready
    };
    let applied_count = entry.applied_files.len();
    let mut actions = row![
        text(format!("{applied_count} file(s) applied to game directory"))
            .size(12)
            .style(warning),
        iced::widget::space::horizontal(),
    ];
    if entry.tool_id == "optiscaler" {
        let (can_activate, activate_disabled_reason) =
            activation_readiness(entry, game_dir_configured, tool_busy, tools_loading);
        let can_deactivate = game_dir_configured && !tool_busy && !tools_loading && entry.enabled;
        actions = actions
            .push(semantics::test_id(
                "tools.optiscaler.activate",
                button(text("Activate").size(12))
                    .style(button::success)
                    .padding([6, 14])
                    .on_action_maybe(
                        can_activate.then_some(ButtonAction::ActivateOptiScaler),
                        activate_disabled_reason,
                    ),
            ))
            .push(semantics::test_id(
                "tools.optiscaler.deactivate",
                button(text("Deactivate").size(12))
                    .style(button::danger)
                    .padding([6, 14])
                    .on_action_maybe(
                        can_deactivate.then_some(ButtonAction::DeactivateOptiScaler),
                        "OptiScaler must be enabled and idle before it can be deactivated.",
                    ),
            ));
    }
    actions = actions
        .push(semantics::test_id(
            format!("tools.{}.apply", entry.tool_id),
            button(text(apply_button_label(entry, tool_busy)).size(12))
                .style(button::primary)
                .padding([6, 14])
                .on_action_maybe(
                    can_apply.then_some(ButtonAction::ApplyTool(entry.tool_id.clone())),
                    apply_disabled_reason,
                ),
        ))
        .push(semantics::test_id(
            format!("tools.{}.revert", entry.tool_id),
            button(text("Revert").size(12))
                .style(button::danger)
                .padding([6, 14])
                .on_action_maybe(
                    can_revert.then_some(ButtonAction::RevertTool(entry.tool_id.clone())),
                    revert_disabled_reason(revert_readiness),
                ),
        ));
    container(actions.spacing(8).align_y(Alignment::Center))
        .padding([8, 12])
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn activation_readiness(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> (bool, &'static str) {
    let (can_apply, apply_disabled_reason) =
        apply_readiness(entry, game_dir_configured, tool_busy, tools_loading);
    if can_apply
        || !entry.enabled && apply_disabled_reason == "All settings are applied. Nothing to deploy."
    {
        return (true, "");
    }
    (false, apply_disabled_reason)
}

#[derive(Debug, Clone, Copy)]
enum RevertReadiness {
    Ready,
    ToolsLoading,
    ToolBusy,
    MissingGameDir,
    NoAppliedFiles,
}

fn revert_disabled_reason(readiness: RevertReadiness) -> &'static str {
    match readiness {
        RevertReadiness::ToolsLoading => "Tool state is still loading.",
        RevertReadiness::ToolBusy => "A tool operation is already in progress.",
        RevertReadiness::MissingGameDir => {
            "Configure the game install path before reverting files."
        }
        RevertReadiness::NoAppliedFiles => {
            "This tool has no applied files to revert for the current game."
        }
        RevertReadiness::Ready => "This tool cannot be reverted right now.",
    }
}

fn apply_readiness(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> (bool, &'static str) {
    if tools_loading {
        return (false, "Tool state is still loading.");
    }
    if tool_busy {
        return (false, "A tool operation is already in progress.");
    }
    if !game_dir_configured {
        return (
            false,
            "Configure the game install path before applying files.",
        );
    }
    if !entry.available {
        return (
            false,
            "Make this tool available on the system before applying files.",
        );
    }
    if !entry.apply_missing_inputs.is_empty() {
        return (false, "Resolve missing source files before applying.");
    }
    if entry.tool_id == "optiscaler" {
        match setting_value_as_string(setting_value(&entry.settings, "source_mode")).as_str() {
            "github_release" | "goverlay_builds" => {
                let tag = setting_value_as_string(setting_value(&entry.settings, "release_tag"));
                let asset =
                    setting_value_as_string(setting_value(&entry.settings, "release_asset"));
                if tag.trim().is_empty() || asset.trim().is_empty() {
                    return (
                        false,
                        "Select an OptiScaler release tag and asset before applying files.",
                    );
                }
            }
            "local_dir" => {
                let path =
                    setting_value_as_string(setting_value(&entry.settings, "local_source_dir"));
                if path.trim().is_empty() {
                    return (
                        false,
                        "Choose a local OptiScaler source directory before applying files.",
                    );
                }
            }
            _ => {}
        }
    }
    if !entry.apply_pending {
        return (false, "All settings are applied. Nothing to deploy.");
    }
    (true, "")
}

fn apply_button_label(entry: &ToolUiEntry, tool_busy: bool) -> String {
    if tool_busy {
        "Applying".to_string()
    } else if !entry.apply_pending && entry.apply_missing_inputs.is_empty() {
        "Up to date".to_string()
    } else {
        let dirty_count = entry.dirty_keys.len();
        if dirty_count > 0 {
            format!("Apply ({dirty_count})")
        } else {
            "Apply".to_string()
        }
    }
}
