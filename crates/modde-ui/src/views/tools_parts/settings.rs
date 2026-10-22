#![allow(clippy::wildcard_imports)]
use super::*;
use iced::widget::column;

pub(super) fn tool_setting_spec<'a>(
    entry: &'a ToolUiEntry,
    key: &str,
) -> Option<&'a modde_games::tools::ToolSettingSpec> {
    entry.setting_specs.iter().find(|spec| spec.key == key)
}

pub(super) fn derived_facts_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    if entry.derived_facts.is_empty() {
        return iced::widget::space::vertical()
            .height(Length::Shrink)
            .into();
    }
    let mut facts = column![text("Detected Game").size(14)].spacing(4);
    for (label, value) in &entry.derived_facts {
        facts = facts.push(
            row![
                text(label.as_str())
                    .size(11)
                    .style(secondary)
                    .width(Length::FillPortion(1)),
                text(value.as_str()).size(11).width(Length::FillPortion(2)),
            ]
            .spacing(8),
        );
    }
    facts.into()
}

pub(super) fn settings_panel(entry: &ToolUiEntry, show_advanced: bool) -> Element<'_, Message> {
    let advanced_count = entry
        .setting_specs
        .iter()
        .filter(|spec| !is_extracted_optiscaler_release_setting(entry, &spec.key))
        .filter(|spec| spec.advanced)
        .count();
    let header = row![
        text("Settings").size(14),
        iced::widget::space::horizontal(),
        if advanced_count > 0 {
            semantics::test_id(
                "tools.settings.toggle_advanced",
                button(
                    text(if show_advanced {
                        "Hide advanced"
                    } else {
                        "Show advanced"
                    })
                    .size(12),
                )
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::ToggleToolAdvancedSettings),
            )
        } else {
            iced::widget::space::horizontal()
                .width(Length::Shrink)
                .into()
        },
    ]
    .align_y(Alignment::Center);
    let mut rows = column![header].spacing(10);
    let mut sections: Vec<(&str, Vec<&modde_games::tools::ToolSettingSpec>)> = Vec::new();
    for spec in &entry.setting_specs {
        if is_extracted_optiscaler_release_setting(entry, &spec.key) {
            continue;
        }
        if spec.advanced && !show_advanced {
            continue;
        }
        if let Some((_, specs)) = sections
            .iter_mut()
            .find(|(section, _)| *section == spec.section)
        {
            specs.push(spec);
        } else {
            sections.push((spec.section, vec![spec]));
        }
    }
    for (section, specs) in sections {
        rows = rows.push(text(section).size(13).style(secondary));
        for spec in specs {
            rows = rows.push(setting_row(entry, spec));
        }
    }
    rows.into()
}

fn is_extracted_optiscaler_release_setting(entry: &ToolUiEntry, key: &str) -> bool {
    entry.tool_id == "optiscaler"
        && matches!(
            key,
            "source_mode"
                | "goverlay_channel"
                | "release_tag"
                | "release_asset"
                | "local_source_dir"
        )
}

pub(super) fn setting_row<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
) -> Element<'a, Message> {
    let control = setting_control(entry, spec);
    let is_dirty = entry.dirty_keys.contains(&*spec.key);

    let label_row = if is_dirty {
        row![
            text("●").size(13).style(warning),
            text(&*spec.label).size(13),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
    } else {
        row![text(&*spec.label).size(13)].align_y(Alignment::Center)
    };

    row![
        column![
            label_row,
            text(&*spec.description).size(11).style(secondary),
        ]
        .spacing(2)
        .width(Length::FillPortion(1)),
        container(control).width(Length::FillPortion(2)),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn setting_control<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
) -> Element<'a, Message> {
    let value = setting_value(&entry.settings, &spec.key);
    let setting_test_id = tool_setting_test_id(&entry.tool_id, &spec.key);
    match &spec.kind {
        ToolSettingKind::Bool => {
            let current = setting_value_as_bool(value).unwrap_or(false);
            semantics::test_id(
                setting_test_id,
                toggler(current)
                    .on_toggle({
                        let tool_id = entry.tool_id.clone();
                        let key = spec.key.to_string();
                        move |enabled| Message::UpdateToolSetting {
                            tool_id: tool_id.clone(),
                            key: key.clone(),
                            value: serde_json::json!(enabled),
                        }
                    })
                    .size(18.0),
            )
        }
        ToolSettingKind::TriStateBool => {
            let selected = tri_state_label(value);
            row![
                tri_state_button(entry, &spec.key, "Auto", &selected),
                tri_state_button(entry, &spec.key, "On", &selected),
                tri_state_button(entry, &spec.key, "Off", &selected),
            ]
            .spacing(6)
            .into()
        }
        ToolSettingKind::Text | ToolSettingKind::Path => {
            text_input(&spec.label, &setting_value_as_string(value))
                .id(semantics::widget_id(setting_test_id))
                .on_input({
                    let tool_id = entry.tool_id.clone();
                    let key = spec.key.to_string();
                    move |input| Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value: serde_json::json!(input),
                    }
                })
                .padding(6)
                .width(Length::Fill)
                .into()
        }
        ToolSettingKind::Select { options } => {
            let selected = value
                .and_then(serde_json::Value::as_str)
                .and_then(|value| options.iter().find(|option| option.value == value).cloned())
                .or_else(|| options.first().cloned());
            semantics::test_id(
                setting_test_id,
                pick_list(options.clone(), selected, {
                    let tool_id = entry.tool_id.clone();
                    let key = spec.key.to_string();
                    move |selected| Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value: serde_json::json!(selected.value),
                    }
                })
                .width(Length::Fill),
            )
        }
        ToolSettingKind::Number { min, max, step } => {
            let current = setting_value_as_f64(value)
                .unwrap_or(*min)
                .clamp(*min, *max);
            semantics::test_id(
                setting_test_id,
                row![
                    slider(*min..=*max, current, {
                        let tool_id = entry.tool_id.clone();
                        let key = spec.key.to_string();
                        move |number| Message::UpdateToolSetting {
                            tool_id: tool_id.clone(),
                            key: key.clone(),
                            value: serde_json::json!(number),
                        }
                    })
                    .step(*step),
                    text(format_number(current))
                        .size(12)
                        .width(Length::Fixed(56.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
        }
        ToolSettingKind::ReadOnly => {
            text(setting_value_as_string(value).if_empty(&spec.description))
                .size(12)
                .style(secondary)
                .into()
        }
    }
}

fn tri_state_button<'a>(
    entry: &'a ToolUiEntry,
    key: &'a str,
    label: &'static str,
    selected: &str,
) -> Element<'a, Message> {
    let style = if selected == label {
        button::primary
    } else {
        button::secondary
    };
    semantics::test_id(
        format!(
            "{}.{}",
            tool_setting_test_id(&entry.tool_id, key),
            label.to_ascii_lowercase()
        ),
        button(text(label).size(12))
            .style(style)
            .padding([4, 10])
            .on_action(ButtonAction::UpdateToolSetting {
                tool_id: entry.tool_id.clone(),
                key: key.to_string(),
                value: tri_state_value(label),
            }),
    )
}

fn tool_setting_test_id(tool_id: &str, key: &str) -> String {
    format!("tools.{tool_id}.setting.{}", key.replace('.', "__"))
}

pub(super) fn setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    settings
        .get(key)
        .or_else(|| nested_setting_value(settings, key))
        .or_else(|| legacy_flat_child_setting_value(settings, key))
}

fn nested_setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let mut current = settings;
    for part in key.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn legacy_flat_child_setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let (root, child) = key.split_once('.')?;
    settings.as_object()?.get(root)?.as_object()?.get(child)
}

fn setting_value_as_bool(value: Option<&serde_json::Value>) -> Option<bool> {
    match value {
        Some(serde_json::Value::Bool(value)) => Some(*value),
        Some(serde_json::Value::String(value)) => parse_bool_string(value),
        _ => None,
    }
}

fn parse_bool_string(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn setting_value_as_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value {
        Some(serde_json::Value::Number(value)) => value.as_f64(),
        Some(serde_json::Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn tri_state_label(value: Option<&serde_json::Value>) -> String {
    match setting_value_as_bool(value) {
        Some(true) => "On".to_string(),
        Some(false) => "Off".to_string(),
        None => "Auto".to_string(),
    }
}

fn tri_state_value(selected: &str) -> serde_json::Value {
    match selected {
        "On" => serde_json::json!(true),
        "Off" => serde_json::json!(false),
        _ => serde_json::json!("auto"),
    }
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

pub(super) fn setting_value_as_string(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(serde_json::Value::Bool(value)) => value.to_string(),
        Some(serde_json::Value::Number(value)) => value.to_string(),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}
