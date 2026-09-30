use std::collections::HashSet;

use crate::views::selectable_text::text;
use iced::widget::{button, column, container, image, mouse_area, pick_list, row};
use iced::{Element, Length, color};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, SidebarGroup, View};
use crate::views::mod_details::ModDetailsState;
use crate::views::save_details::SaveDetailsState;

struct NavItem {
    label: &'static str,
    target: NavTarget,
}

struct NavGroup {
    group: SidebarGroup,
    items: &'static [NavItem],
}

#[derive(Clone, Copy)]
enum NavTarget {
    Library,
    ModList,
    Saves,
    DataTab,
    BrowseNexus,
    Collections,
    Wabbajack,
    Downloads,
    Diagnostics,
    Tools,
    Executables,
    Settings,
}

impl NavTarget {
    fn view(self) -> View {
        match self {
            NavTarget::Library => View::Library,
            NavTarget::ModList => View::ModList,
            NavTarget::Saves => View::Saves,
            NavTarget::DataTab => View::DataTab,
            NavTarget::BrowseNexus => View::BrowseNexus,
            NavTarget::Collections => View::Collections,
            NavTarget::Wabbajack => View::WabbajackInstaller(Default::default()),
            NavTarget::Downloads => View::Downloads,
            NavTarget::Diagnostics => View::Diagnostics,
            NavTarget::Tools => View::Tools,
            NavTarget::Executables => View::Executables,
            NavTarget::Settings => View::Settings,
        }
    }
}

const GAME_ITEMS: &[NavItem] = &[
    NavItem {
        label: "Library",
        target: NavTarget::Library,
    },
    NavItem {
        label: "Mod List",
        target: NavTarget::ModList,
    },
    NavItem {
        label: "Saves",
        target: NavTarget::Saves,
    },
    NavItem {
        label: "Data Files",
        target: NavTarget::DataTab,
    },
    NavItem {
        label: "Diagnostics",
        target: NavTarget::Diagnostics,
    },
    NavItem {
        label: "Tools",
        target: NavTarget::Tools,
    },
    NavItem {
        label: "Executables",
        target: NavTarget::Executables,
    },
];

const INSTALL_ITEMS: &[NavItem] = &[
    NavItem {
        label: "Browse Nexus",
        target: NavTarget::BrowseNexus,
    },
    NavItem {
        label: "Collections",
        target: NavTarget::Collections,
    },
    NavItem {
        label: "Wabbajack",
        target: NavTarget::Wabbajack,
    },
    NavItem {
        label: "Downloads",
        target: NavTarget::Downloads,
    },
];

const GENERAL_ITEMS: &[NavItem] = &[NavItem {
    label: "Settings",
    target: NavTarget::Settings,
}];

const NAV_GROUPS: &[NavGroup] = &[
    NavGroup {
        group: SidebarGroup::Game,
        items: GAME_ITEMS,
    },
    NavGroup {
        group: SidebarGroup::Install,
        items: INSTALL_ITEMS,
    },
    NavGroup {
        group: SidebarGroup::General,
        items: GENERAL_ITEMS,
    },
];

/// Render the navigation sidebar.
pub fn view<'a>(
    active_view: &View,
    collapsed_groups: &HashSet<SidebarGroup>,
    profiles: &'a [modde_core::profile::ProfileSummary],
    active_profile: &'a Option<String>,
    experiment_depth: usize,
    save_profiles_supported: bool,
    mod_details: Option<&'a ModDetailsState>,
    save_details: Option<&'a SaveDetailsState>,
) -> Element<'a, Message> {
    let nav_button = |label: &'static str, target: View, current: &View| -> Element<'a, Message> {
        let is_active = std::mem::discriminant(&target) == std::mem::discriminant(current);
        let btn = button(text(label).size(14))
            .width(Length::Fill)
            .padding([6, 12]);
        if is_active {
            btn.style(button::primary)
                .described_disabled("This section is already open.")
        } else {
            btn.style(button::secondary)
                .on_action(ButtonAction::SwitchView(target))
        }
    };

    let mut nav = column![].spacing(6);
    for group in NAV_GROUPS {
        nav = nav.push(render_group_header(
            group.group,
            collapsed_groups.contains(&group.group),
        ));
        let contains_active = group
            .items
            .iter()
            .any(|item| same_view_kind(&item.target.view(), active_view));
        let show_all_items = !collapsed_groups.contains(&group.group);
        if show_all_items || contains_active {
            let mut group_items = column![].spacing(4);
            for item in group.items {
                let view = item.target.view();
                if matches!(item.target, NavTarget::Saves)
                    && !save_profiles_supported
                    && !same_view_kind(&view, active_view)
                {
                    continue;
                }
                if show_all_items || same_view_kind(&view, active_view) {
                    group_items = group_items.push(nav_button(item.label, view, active_view));
                }
            }
            nav = nav.push(group_items);
        }
    }

    // ── Profile section ──
    let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    let profile_selector = column![
        text("Profile").size(12),
        pick_list(
            profile_names,
            active_profile.clone(),
            Message::SwitchProfile,
        )
        .width(Length::Fill)
        .placeholder("No profiles"),
    ]
    .spacing(4);

    // ── Profile actions ──
    let mut profile_actions = row![].spacing(4);
    if let Some(name) = active_profile {
        let name_del = name.clone();
        profile_actions = profile_actions.push(
            button(text("Del").size(11))
                .style(button::danger)
                .padding([3, 8])
                .on_action(ButtonAction::DeleteProfile(name_del)),
        );
    }
    profile_actions = profile_actions.push(
        button(text("New").size(11))
            .style(button::success)
            .padding([3, 8])
            .on_action(ButtonAction::OpenNewProfileDialog),
    );
    if let Some(name) = active_profile {
        let name_fork = name.clone();
        profile_actions = profile_actions.push(
            button(text("Fork").size(11))
                .style(button::secondary)
                .padding([3, 8])
                .on_action(ButtonAction::ForkProfile {
                    source: name_fork.clone(),
                    new_name: format!("{name_fork}-fork"),
                }),
        );
    }

    // ── Experiment indicator ──
    let mut sections = column![
        nav,
        iced::widget::rule::horizontal(1),
        profile_selector,
        profile_actions,
    ]
    .spacing(10)
    .padding(12)
    .width(Length::Fixed(190.0));

    if experiment_depth > 0 {
        let experiment_section = column![
            text(format!("Experiment (depth {experiment_depth})"))
                .size(12)
                .color(color!(0xFFAA44)),
            row![
                button(text("Rollback").size(11))
                    .style(button::danger)
                    .padding([3, 8])
                    .on_action(ButtonAction::RollbackExperiment),
                button(text("Commit").size(11))
                    .style(button::success)
                    .padding([3, 8])
                    .on_action(ButtonAction::CommitExperiment),
            ]
            .spacing(4),
        ]
        .spacing(4);

        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(experiment_section);
    } else if active_profile.is_some() {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(
            button(text("Try Profile").size(11))
                .style(button::secondary)
                .padding([3, 8])
                .width(Length::Fill)
                .on_action(ButtonAction::TryProfile),
        );
    }

    // ── Detail panel (mod details or save details, mutually exclusive) ──
    if let Some(details) = mod_details {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(render_mod_details(details));
    } else if let Some(details) = save_details {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(render_save_details(details));
    }

    iced::widget::row![
        iced::widget::scrollable(container(sections).style(container::rounded_box))
            .height(Length::Fill),
        iced::widget::rule::vertical(1),
    ]
    .into()
}

fn same_view_kind(a: &View, b: &View) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn render_group_header(group: SidebarGroup, collapsed: bool) -> Element<'static, Message> {
    let icon = if collapsed { ">" } else { "v" };
    button(row![text(icon).size(12), text(group.label()).size(12)].spacing(6))
        .style(button::text)
        .padding([2, 4])
        .width(Length::Fill)
        .on_action(ButtonAction::ToggleSidebarGroup(group))
}

/// Maximum character count for the mod summary text before it is
/// truncated with an ellipsis. ~160 chars is roughly 4-5 wrapped lines
/// at the sidebar width.
const SUMMARY_MAX: usize = 160;

/// Render the mod detail panel appended to the bottom of the left sidebar.
/// Width budget is ~166px (190px sidebar minus 12px padding each side minus
/// a little breathing room), so text is sized small and the thumbnail is
/// clamped to `Length::Fill` within that column.

#[path = "sidebar_parts/details.rs"]
mod details;

use self::details::{render_mod_details, render_save_details};
