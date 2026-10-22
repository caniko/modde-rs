use crate::views::selectable_text::{danger, primary, success, text, warning};
use iced::widget::{
    button, checkbox, column, container, image, progress_bar, radio, row, scrollable,
};
use iced::{Element, Length};

use fomod_oxide::config::GroupType;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, Modde};

/// Render the FOMOD wizard view from the live installer state.
pub fn view(app: &Modde) -> Element<'_, Message> {
    let installer = match app.fomod_installer.as_ref() {
        Some(i) => i,
        None => {
            return column![text("No active FOMOD installer.").size(16)]
                .spacing(10)
                .into();
        }
    };

    let visible_steps = installer.visible_steps();
    let total_visible = visible_steps.len();

    // Find the current step.
    let current_entry = app
        .fomod_visible_step_indices
        .get(app.fomod_wizard_pos)
        .and_then(|&step_idx| {
            visible_steps
                .iter()
                .find(|&&(idx, _)| idx == step_idx)
                .map(|&(idx, step)| (idx, step))
        });

    let (step_idx, step) = match current_entry {
        Some(entry) => entry,
        None => {
            return column![text("No visible steps available.").size(16)]
                .spacing(10)
                .into();
        }
    };

    // Module name from the config.
    let Some(config) = installer.config() else {
        return column![text("No FOMOD config available.").size(16)]
            .spacing(10)
            .into();
    };
    let module_name = &config.module_name.value;

    // ── Header: module image, name, step progress, completion bar ──
    let mut header = column![].spacing(5);

    // Module header image
    if let Some(img_path) = installer.module_image_path()
        && let Some(ref source_dir) = app.fomod_source_dir
        && let Some(resolved) = installer.resolve_image(source_dir, img_path)
    {
        header = header.push(
            image(image::Handle::from_path(resolved))
                .width(Length::Fill)
                .height(Length::Fixed(120.0)),
        );
    }

    header = header.push(text(module_name).size(24));
    header = header.push(
        text(format!(
            "Step {} of {}: {}",
            app.fomod_wizard_pos + 1,
            total_visible,
            step.name,
        ))
        .size(18),
    );

    // Completion progress bar
    let status = installer.completion_status();
    header = header.push(
        row![
            progress_bar(0.0..=1.0, status.fraction()).girth(6),
            text(format!(
                "{}/{} groups complete",
                status.satisfied_groups, status.total_groups
            ))
            .size(11),
        ]
        .spacing(8),
    );

    // Validation hints for the current step
    let hints = installer.validate_step(step_idx);
    if !hints.is_empty() {
        for hint in &hints {
            header = header.push(text(hint.to_string()).size(11).style(warning));
        }
    }

    // File conflict warnings
    if !app.fomod_conflicts.is_empty() {
        header = header.push(
            text(format!(
                "{} file conflict(s) detected",
                app.fomod_conflicts.len()
            ))
            .size(11)
            .style(danger),
        );
    }

    // ── Build group UI ──
    let mut groups_col = column![].spacing(15);

    if let Some(ref file_groups) = step.optional_file_groups {
        for (group_idx, group) in file_groups.groups.iter().enumerate() {
            let group_type_label = match group.group_type {
                GroupType::SelectExactlyOne => "Select exactly one",
                GroupType::SelectAtMostOne => "Select at most one",
                GroupType::SelectAtLeastOne => "Select at least one",
                GroupType::SelectAll => "All required",
                GroupType::SelectAny => "Select any",
            };

            let mut group_col =
                column![text(&group.name).size(16), text(group_type_label).size(12),].spacing(5);

            let current_sel = app
                .fomod_selections
                .get(&(step_idx, group_idx))
                .cloned()
                .unwrap_or_default();

            let is_radio = matches!(
                group.group_type,
                GroupType::SelectExactlyOne | GroupType::SelectAtMostOne
            );

            for (plugin_idx, plugin) in group.plugins.plugins.iter().enumerate() {
                let is_selected = current_sel.contains(&plugin_idx);

                // Plugin type badge
                let plugin_type = installer
                    .plugin_type_at(step_idx, group_idx, plugin_idx)
                    .unwrap_or(fomod_oxide::config::PluginType::Optional);
                let type_badge = match plugin_type {
                    fomod_oxide::config::PluginType::Required => Some("[Required]"),
                    fomod_oxide::config::PluginType::Recommended => Some("[Recommended]"),
                    fomod_oxide::config::PluginType::NotUsable => Some("[Not Usable]"),
                    fomod_oxide::config::PluginType::CouldBeUsable => Some("[May Work]"),
                    fomod_oxide::config::PluginType::Optional => None,
                };

                let label = if let Some(badge) = type_badge {
                    format!("{} {}", plugin.name, badge)
                } else {
                    plugin.name.clone()
                };

                let plugin_widget: Element<'_, Message> = if is_radio {
                    let chosen: Option<usize> = current_sel.first().copied();
                    radio(&label, plugin_idx, chosen, move |picked| {
                        Message::FOMODChoice {
                            step: step_idx,
                            group: group_idx,
                            option: picked,
                            selected: true,
                        }
                    })
                    .into()
                } else if group.group_type == GroupType::SelectAll {
                    checkbox(true).label(label.clone()).into()
                } else {
                    let si = step_idx;
                    let gi = group_idx;
                    let pi = plugin_idx;
                    checkbox(is_selected)
                        .label(label.clone())
                        .on_toggle(move |checked| Message::FOMODChoice {
                            step: si,
                            group: gi,
                            option: pi,
                            selected: checked,
                        })
                        .into()
                };

                let mut option_col = column![plugin_widget].spacing(2);

                // Plugin image
                if let Some(img_path) = installer.plugin_image_path(step_idx, group_idx, plugin_idx)
                    && let Some(ref source_dir) = app.fomod_source_dir
                    && let Some(resolved) = installer.resolve_image(source_dir, img_path)
                    && is_selected
                {
                    option_col = option_col.push(
                        image(image::Handle::from_path(resolved))
                            .width(Length::Fixed(200.0))
                            .height(Length::Fixed(120.0)),
                    );
                }

                // Description
                if let Some(ref desc) = plugin.description
                    && !desc.is_empty()
                {
                    option_col = option_col.push(text(desc).size(11));
                }

                // File preview count for selected plugins
                if is_selected {
                    let preview = installer.preview_plugin(step_idx, group_idx, plugin_idx);
                    if !preview.is_empty() {
                        option_col = option_col.push(
                            text(format!("{} file(s) to install", preview.len()))
                                .size(10)
                                .style(success),
                        );
                    }
                }

                group_col = group_col.push(option_col);
            }

            groups_col = groups_col.push(container(group_col).padding(10).width(Length::Fill));
        }
    }

    // ── Install plan preview summary ──
    let preview = installer.preview_current();
    if !preview.operations.is_empty() {
        groups_col = groups_col.push(
            text(format!(
                "Total: {} file operation(s) queued",
                preview.operations.len()
            ))
            .size(12)
            .style(primary),
        );
    }

    // ── Navigation buttons ──
    let mut nav = row![].spacing(10);

    nav = nav.push(button(text("Cancel")).on_action(ButtonAction::FomodCancel));

    // Undo button
    if app.fomod_can_undo {
        nav = nav.push(button(text("Undo")).on_action(ButtonAction::FomodUndo));
    }

    if app.fomod_wizard_pos > 0 {
        nav = nav.push(button(text("Back")).on_action(ButtonAction::FomodBack));
    }

    let is_last = app.fomod_is_last_step();
    let next_label = if is_last { "Install" } else { "Next" };

    // Disable Install button if not ready
    let next_btn = button(text(next_label)).on_action_maybe(
        (!is_last || installer.is_ready_to_install()).then_some(ButtonAction::FomodNext),
        "Complete the required FOMOD choices before installing.",
    );
    nav = nav.push(next_btn);

    let content = column![header, scrollable(groups_col).height(Length::Fill), nav,]
        .spacing(15)
        .width(Length::Fill)
        .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(10)
        .into()
}
