use std::collections::{HashMap, HashSet};

use crate::views::selectable_text::text;
use iced::widget::{button, checkbox, column, container, row, scrollable, text_input};
use iced::{Alignment, Element, Length};

use modde_core::filter::{self, FilterCriterion, FilterKind, FilterMode, TriState};
use modde_core::profile::EnabledMod;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

// ─── Constants ────────────────────────────────────────────────────

const UNCATEGORIZED_LABEL: &str = "Uncategorized";

// ─── View function ────────────────────────────────────────────────

/// Render the mod list view with filter toolbar and collapsible category separators.
///
/// `profile_locked` is `true` when the containing profile carries a
/// `Profile::load_order_lock`; it disables *all* reorder buttons at the
/// view layer, complementing the `Message::ReorderMod` handler's own
/// refusal check (defense in depth — the view won't let the user try a
/// gesture the handler will reject).
pub fn view_filtered<'a>(
    mods: &'a [EnabledMod],
    mod_id_filter_keys: &'a [String],
    filter_text: &'a str,
    selected_index: Option<usize>,
    filter_mode: FilterMode,
    active_filters: &'a [FilterCriterion],
    collapsed_categories: &'a HashSet<Option<i64>>,
    categories: &'a [(Option<i64>, String)],
    compact: bool,
    profile_locked: bool,
) -> Element<'a, Message> {
    // ── Action toolbar ──
    let toolbar = row![
        button(text("Add Mod").size(14))
            .style(button::primary)
            .padding([6, 14])
            .on_action(ButtonAction::AddMod),
        button(text("Remove").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action_maybe(
                selected_index.map(|index| {
                    ButtonAction::RemoveMod(index)
                        .confirmed("Remove mod", "Remove the selected mod from this profile?")
                }),
                "Select a mod before removing it from the active profile.",
            ),
        iced::widget::space::horizontal(),
        button(text("Deploy").size(14))
            .style(button::success)
            .padding([6, 14])
            .on_action(ButtonAction::Deploy),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // ── Filter toolbar ──
    let search = text_input("Filter mods...", filter_text)
        .id(crate::semantics::widget_id("mod_list.filter"))
        .on_input(Message::FilterChanged)
        .padding(6)
        .width(Length::Fill);

    let mode_label = filter_mode.label();
    let mode_btn = button(text(mode_label).size(11))
        .style(if filter_mode == FilterMode::And {
            button::primary
        } else {
            button::secondary
        })
        .padding([3, 8])
        .on_action(ButtonAction::ToggleFilterMode);

    let filter_buttons = row![
        mode_btn,
        tri_state_button(
            "Enabled",
            FilterKind::Enabled,
            find_filter_state(active_filters, FilterKind::Enabled)
        ),
        tri_state_button(
            "Notes",
            FilterKind::HasNotes,
            find_filter_state(active_filters, FilterKind::HasNotes)
        ),
        tri_state_button(
            "Nexus",
            FilterKind::HasNexusId,
            find_filter_state(active_filters, FilterKind::HasNexusId)
        ),
        button(text("Clear").size(11))
            .style(button::secondary)
            .padding([3, 8])
            .on_action(ButtonAction::ClearFilters),
        iced::widget::space::horizontal(),
        button(text(if compact { "Normal" } else { "Compact" }).size(11))
            .style(button::text)
            .padding([3, 8])
            .on_action(ButtonAction::ToggleCompactModList),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let filter_toolbar = column![search, filter_buttons].spacing(4);

    // ── Column header ──
    let header = row![
        text("").width(Length::Fixed(32.0)),
        text("Enabled").size(12).width(Length::Fixed(60.0)),
        text("Mod Name").size(12).width(Length::Fill),
        text("Version").size(12).width(Length::Fixed(80.0)),
        text("Reorder").size(12).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    // ── Apply filters ──
    let filtered_indices = filter::apply_filters_with_mod_id_keys(
        mods,
        mod_id_filter_keys,
        filter_text,
        active_filters,
        filter_mode,
    );

    let total_shown = filtered_indices.len();

    // ── Group by category ──
    let category_map: HashMap<Option<i64>, &str> = categories
        .iter()
        .map(|(id, name)| (*id, name.as_str()))
        .collect();

    let mut grouped: Vec<(Option<i64>, &str, Vec<usize>)> =
        build_category_groups(&filtered_indices, mods, &category_map);

    // Sort: uncategorized (None) first, then by category name
    grouped.sort_by(|a, b| {
        if a.0.is_none() {
            std::cmp::Ordering::Less
        } else if b.0.is_none() {
            std::cmp::Ordering::Greater
        } else {
            a.1.cmp(b.1)
        }
    });

    // ── Build rows ──
    let mod_rows: Element<Message> = if filtered_indices.is_empty() {
        container(
            text(if mods.is_empty() {
                "No mods found. Click 'Add Mod' to get started."
            } else {
                "No mods match the current filter."
            })
            .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else if categories.is_empty() {
        // No categories defined — flat list
        let rows = build_flat_mod_rows(
            &filtered_indices,
            mods,
            selected_index,
            compact,
            profile_locked,
        );
        scrollable(rows).height(Length::Fill).into()
    } else {
        // Categorized list with collapsible separators
        let rows = build_categorized_rows(
            &grouped,
            mods,
            selected_index,
            collapsed_categories,
            compact,
            profile_locked,
        );
        scrollable(rows).height(Length::Fill).into()
    };

    let status = text(format!("{total_shown} mod(s) shown")).size(12);

    column![
        toolbar,
        filter_toolbar,
        header,
        iced::widget::rule::horizontal(1),
        mod_rows,
        status,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Find the current tri-state for a given filter kind.
fn find_filter_state(criteria: &[FilterCriterion], kind: FilterKind) -> TriState {
    criteria
        .iter()
        .find(|c| c.kind == kind)
        .map_or(TriState::Ignore, |c| c.state)
}

/// Build a tri-state toggle button.
fn tri_state_button(label: &str, kind: FilterKind, state: TriState) -> Element<'_, Message> {
    let prefix = state.label();
    let display = format!("{prefix} {label}");
    let style = match state {
        TriState::Ignore => button::text,
        TriState::Include => button::success,
        TriState::Exclude => button::danger,
    };
    button(text(display).size(11))
        .style(style)
        .padding([3, 8])
        .on_action(ButtonAction::CycleFilter(kind))
}

/// Group filtered mod indices by category.
fn build_category_groups<'a>(
    filtered_indices: &[usize],
    mods: &'a [EnabledMod],
    category_map: &HashMap<Option<i64>, &'a str>,
) -> Vec<(Option<i64>, &'a str, Vec<usize>)> {
    let mut groups: HashMap<Option<i64>, Vec<usize>> = HashMap::new();
    for &idx in filtered_indices {
        let cat_id = mods[idx].category_id;
        groups.entry(cat_id).or_default().push(idx);
    }

    groups
        .into_iter()
        .map(|(cat_id, indices)| {
            let name = category_map
                .get(&cat_id)
                .copied()
                .unwrap_or(if cat_id.is_none() {
                    UNCATEGORIZED_LABEL
                } else {
                    "Unknown"
                });
            (cat_id, name, indices)
        })
        .collect()
}

/// Build a flat list of mod rows (no category separators).
fn build_flat_mod_rows<'a>(
    indices: &[usize],
    mods: &'a [EnabledMod],
    selected_index: Option<usize>,
    compact: bool,
    profile_locked: bool,
) -> iced::widget::Column<'a, Message> {
    indices.iter().fold(column![].spacing(2), |col, &idx| {
        col.push(mod_row(
            idx,
            &mods[idx],
            selected_index,
            mods.len(),
            compact,
            profile_locked,
        ))
    })
}

/// Build categorized rows with collapsible separators.
fn build_categorized_rows<'a>(
    groups: &[(Option<i64>, &str, Vec<usize>)],
    mods: &'a [EnabledMod],
    selected_index: Option<usize>,
    collapsed: &HashSet<Option<i64>>,
    compact: bool,
    profile_locked: bool,
) -> iced::widget::Column<'a, Message> {
    let mut col = column![].spacing(2);

    for (cat_id, cat_name, indices) in groups {
        let is_collapsed = collapsed.contains(cat_id);
        let toggle_icon = if is_collapsed { ">" } else { "v" };
        let count_label = format!("{} ({} mods)", cat_name, indices.len());

        let separator = button(
            row![text(toggle_icon).size(12), text(count_label).size(12),]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .style(button::text)
        .padding([4, 8])
        .width(Length::Fill)
        .on_action(ButtonAction::ToggleSeparator(*cat_id));

        col = col.push(separator);
        col = col.push(iced::widget::rule::horizontal(1));

        if !is_collapsed {
            for &idx in indices {
                col = col.push(mod_row(
                    idx,
                    &mods[idx],
                    selected_index,
                    mods.len(),
                    compact,
                    profile_locked,
                ));
            }
        }
    }

    col
}

/// Render a single mod row.
///
/// `profile_locked` disables the reorder buttons for *every* row when the
/// containing profile has a `Profile::load_order_lock`. `entry.lock`
/// disables only this one row (per-mod pin), independent of the profile
/// lock.
fn mod_row(
    idx: usize,
    entry: &EnabledMod,
    selected_index: Option<usize>,
    total: usize,
    compact: bool,
    profile_locked: bool,
) -> Element<'_, Message> {
    let is_selected = selected_index == Some(idx);
    let font_size: f32 = if compact { 12.0 } else { 14.0 };
    let row_pad: u16 = if compact { 2 } else { 4 };

    let row_blocked = profile_locked || entry.lock.is_some();

    let up_btn = button(text("^").size(12)).padding([2, 6]).on_action_maybe(
        if !row_blocked && idx > 0 {
            Some(ButtonAction::ReorderMod {
                mod_id: entry.mod_id.clone(),
                direction: crate::app::ReorderDirection::Up,
            })
        } else {
            None
        },
        "This mod cannot move up because it is first, pinned, or the profile load order is locked.",
    );

    let down_btn = button(text("v").size(12))
        .padding([2, 6])
        .on_action_maybe(
            if !row_blocked && idx < total - 1 {
                Some(ButtonAction::ReorderMod {
                    mod_id: entry.mod_id.clone(),
                    direction: crate::app::ReorderDirection::Down,
                })
            } else {
                None
            },
            "This mod cannot move down because it is last, pinned, or the profile load order is locked.",
        );

    let priority = text(format!("{:>3}", idx + 1))
        .size(12)
        .width(Length::Fixed(32.0));

    let cb = checkbox(entry.enabled).on_toggle({
        let mod_id = entry.mod_id.clone();
        move |val| Message::ToggleMod {
            mod_id: mod_id.clone(),
            enabled: val,
        }
    });

    // Prefix per-mod-pinned rows with a marker, matching load_order.rs.
    let label_owned: String = {
        let base = entry.display_name.as_deref().unwrap_or(&entry.mod_id);
        if entry.lock.is_some() {
            format!("[pinned] {base}")
        } else {
            base.to_string()
        }
    };
    let name = button(text(label_owned).size(font_size))
        .style(if is_selected {
            button::primary
        } else {
            button::text
        })
        .padding([2, 4])
        .on_action(ButtonAction::SelectMod(idx));

    let version_str = entry.version.as_deref().unwrap_or("-");
    let version = text(version_str).size(12).width(Length::Fixed(80.0));

    row![
        priority,
        container(cb).width(Length::Fixed(60.0)),
        container(name).width(Length::Fill),
        version,
        row![up_btn, down_btn].spacing(2).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([row_pad, 8])
    .into()
}
