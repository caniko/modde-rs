use crate::views::selectable_text::{
    TextStyle, danger, primary, secondary, success, text, warning,
};
use iced::widget::{
    button, checkbox, column, container, progress_bar, row, scrollable, text_input,
};
use iced::{Alignment, Element, Length};

use modde_core::manifest::wabbajack::WabbajackManifest;
use modde_sources::wabbajack::catalog::{
    CatalogEntrySource, CatalogFilter, WabbajackCatalogEntry, filter_entries,
};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, WabbajackInstallerState, WabbajackTab};
use crate::semantics;
use crate::views::game_picker::{GameOption, game_pick_list, wabbajack_game_options};
use crate::views::tabs::{Tab, tab_bar};

pub fn view<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
    _available_games: &'a [(String, String)],
    current_game_id: Option<&'a str>,
) -> Element<'a, Message> {
    let title_bar = row![
        text("Wabbajack Installer").size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(12))
            .padding([4, 10])
            .on_action(ButtonAction::LoadWabbajackCatalog),
    ]
    .align_y(Alignment::Center);

    let tabs = tab_bar([
        Tab::new(
            "Catalog",
            state.tab == WabbajackTab::Catalog,
            ButtonAction::WabbajackTabChanged(WabbajackTab::Catalog),
        )
        .test_id("wabbajack.tab.catalog"),
        Tab::new(
            "Authored Files",
            state.tab == WabbajackTab::AuthoredFiles,
            ButtonAction::WabbajackTabChanged(WabbajackTab::AuthoredFiles),
        )
        .test_id("wabbajack.tab.authored_files"),
        Tab::new(
            "Manual",
            state.tab == WabbajackTab::Manual,
            ButtonAction::WabbajackTabChanged(WabbajackTab::Manual),
        )
        .test_id("wabbajack.tab.manual"),
    ]);

    let manifest = manifest.as_ref();
    let content = match state.tab {
        WabbajackTab::Catalog | WabbajackTab::AuthoredFiles => {
            explorer_tab(state, manifest, current_game_id)
        }
        WabbajackTab::Manual => manual_tab(state, manifest),
    };

    column![title_bar, tabs, iced::widget::rule::horizontal(1), content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn explorer_tab<'a>(
    state: &'a WabbajackInstallerState,
    manifest: Option<&'a WabbajackManifest>,
    current_game_id: Option<&'a str>,
) -> Element<'a, Message> {
    let source = match state.tab {
        WabbajackTab::Catalog => CatalogEntrySource::Official,
        WabbajackTab::AuthoredFiles => CatalogEntrySource::Authored,
        WabbajackTab::Manual => CatalogEntrySource::Official,
    };

    let mut games: Vec<GameOption> = wabbajack_game_options(&state.entries, &source);
    if let Some(game_id) = current_game_id
        && !games.iter().any(|option| option.value == game_id)
    {
        games.push(GameOption::from_game_id(game_id.to_string()));
        games.sort_by_key(std::string::ToString::to_string);
    }
    games.insert(0, GameOption::new("", "All games"));
    let selected_game = state
        .game_filter
        .as_ref()
        .and_then(|game| games.iter().find(|option| option.value == *game).cloned())
        .or_else(|| games.first().cloned());

    let filtered_base: Vec<WabbajackCatalogEntry> = state
        .entries
        .iter()
        .filter(|entry| entry.source == source)
        .cloned()
        .collect();
    let filtered = filter_entries(
        &filtered_base,
        &CatalogFilter {
            query: (!state.search.is_empty()).then(|| state.search.clone()),
            game: state.game_filter.clone(),
            official_only: state.official_only,
            include_nsfw: state.include_nsfw,
            include_down: state.include_down,
        },
    );

    let toolbar = column![
        row![
            text_input("Search modlists...", &state.search)
                .id(semantics::widget_id("wabbajack.input.search"))
                .on_input(Message::WabbajackSearchChanged)
                .padding(6)
                .width(Length::Fill),
            game_pick_list(games, selected_game, "All games", |option| {
                Message::WabbajackGameFilterChanged(
                    (!option.value.is_empty()).then_some(option.value),
                )
            },),
        ]
        .spacing(8),
        row![
            checkbox(state.official_only)
                .label("Official only")
                .on_toggle(Message::WabbajackToggleOfficialOnly),
            checkbox(state.include_nsfw)
                .label("NSFW")
                .on_toggle(Message::WabbajackToggleNsfw),
            checkbox(state.include_down)
                .label("Unavailable")
                .on_toggle(Message::WabbajackToggleDown),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .spacing(6);

    let list = if state.loading {
        column![text("Loading catalogs...").size(14)]
    } else if let Some(error) = &state.error {
        column![
            text(format!("Catalog load failed: {error}"))
                .size(13)
                .style(danger)
        ]
    } else if filtered.is_empty() {
        column![text("No Wabbajack entries match the current filters.").size(13)]
    } else {
        filtered
            .into_iter()
            .fold(column![].spacing(6), |col, entry| {
                let index = state
                    .entries
                    .iter()
                    .position(|candidate| candidate.download_url == entry.download_url)
                    .unwrap_or(0);
                col.push(entry_row(entry, index, state.selected_index == Some(index)))
            })
    };

    let detail = selected_entry(state).map_or_else(
        || detail_panel(state, manifest, None),
        |entry| detail_panel(state, manifest, Some(entry)),
    );

    row![
        column![
            toolbar,
            scrollable(list).height(Length::Fill),
            text(&state.status).size(12),
        ]
        .spacing(8)
        .width(Length::FillPortion(2)),
        detail.width(Length::FillPortion(1)),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

fn manual_tab<'a>(
    state: &'a WabbajackInstallerState,
    manifest: Option<&'a WabbajackManifest>,
) -> Element<'a, Message> {
    column![
        text_input(
            "URL, machine URL, title, or local path...",
            &state.manual_source
        )
        .id(semantics::widget_id("wabbajack.input.manual_source"))
        .on_input(Message::WabbajackManualSourceChanged)
        .padding(6)
        .width(Length::Fill),
        row![
            semantics::test_id(
                "wabbajack.select_file",
                button(text("Select .wabbajack File").size(13))
                    .style(button::secondary)
                    .padding([6, 12])
                    .on_action(ButtonAction::OpenWabbajackFile),
            ),
            semantics::test_id(
                "wabbajack.manual.download",
                button(text("Download").size(13))
                    .style(button::primary)
                    .padding([6, 12])
                    .on_action(ButtonAction::WabbajackDownloadSelected),
            ),
            semantics::test_id(
                "wabbajack.manual.install",
                button(text("Install").size(13))
                    .style(button::success)
                    .padding([6, 12])
                    .on_action_maybe(
                        state
                            .can_install()
                            .then_some(ButtonAction::WabbajackStartInstall),
                        "Resolve readiness blockers before installing.",
                    ),
            ),
        ]
        .spacing(8),
        detail_panel(state, manifest, selected_entry(state)).width(Length::Fill),
    ]
    .spacing(10)
    .height(Length::Fill)
    .into()
}

fn entry_row<'a>(
    entry: WabbajackCatalogEntry,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let source = match entry.source {
        CatalogEntrySource::Official => "official",
        CatalogEntrySource::Authored => "authored",
    };
    let mut meta = vec![source.to_string()];
    if let Some(game) = &entry.game {
        meta.push(game.clone());
    }
    if let Some(version) = &entry.version {
        meta.push(format!("v{version}"));
    }
    if let Some(repository) = &entry.repository_name {
        meta.push(repository.clone());
    }
    if entry.force_down {
        meta.push("down".to_string());
    }
    let row = column![
        text(entry.title).size(14),
        text(meta.join(" · ")).size(11).style(secondary),
    ]
    .spacing(2);
    let btn = button(row).width(Length::Fill).padding(8);
    let btn = if selected {
        btn.style(button::primary)
    } else {
        btn.style(button::secondary)
    };
    semantics::test_id(
        format!("wabbajack.entry.{index}"),
        btn.on_action(ButtonAction::WabbajackSelectEntry(index)),
    )
}

fn detail_panel<'a>(
    state: &'a WabbajackInstallerState,
    manifest: Option<&'a WabbajackManifest>,
    entry: Option<&'a WabbajackCatalogEntry>,
) -> container::Container<'a, Message> {
    let mut details = column![text("Selected Modlist").size(16)].spacing(8);
    if let Some(entry) = entry {
        details = details
            .push(text(&entry.title).size(15))
            .push(
                text(format!(
                    "Author: {}",
                    entry
                        .author
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string())
                ))
                .size(12),
            )
            .push(
                text(format!(
                    "Game: {}",
                    entry.game.clone().unwrap_or_else(|| "unknown".to_string())
                ))
                .size(12),
            )
            .push(text(format_size_summary(entry)).size(12));
        if let Some(machine) = &entry.machine_url {
            let id = entry.repository_name.as_ref().map_or_else(
                || machine.clone(),
                |repository| format!("{repository}/{machine}"),
            );
            details = details.push(text(format!("ID: {id}")).size(12));
        }
        if let Some(readme) = &entry.readme_url {
            details = details.push(
                button(text("Open Readme").size(12))
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action(ButtonAction::WabbajackOpenUrl(readme.clone())),
            );
        }
        if let Some(image) = &entry.image_url {
            details = details.push(text(format!("Image: {image}")).size(11));
        }
    } else if let Some(manifest) = manifest {
        details = details
            .push(text(&manifest.name).size(15))
            .push(text(format!("Author: {}", manifest.author)).size(12))
            .push(text(format!("Game: {}", manifest.game)).size(12))
            .push(text(format!("Version: {}", manifest.version)).size(12))
            .push(
                text(format!(
                    "{} archive(s), {} directive(s)",
                    manifest.archives.len(),
                    manifest.directives.len()
                ))
                .size(12),
            );
    } else {
        details = details.push(text("Select, download, or open a .wabbajack file.").size(12));
    }

    let file = state.file_path.as_ref().map_or_else(
        || "No local file selected".to_string(),
        |p| p.display().to_string(),
    );

    details = details
        .push(iced::widget::rule::horizontal(1))
        .push(text(file).size(11))
        .push(action_controls(state))
        .push(target_controls(state))
        .push(readiness_panel(state))
        .push(missing_archives_panel(state))
        .push(progress_panel(state))
        .push(hm_controls(state));

    container(scrollable(details).height(Length::Fill))
        .padding(10)
        .height(Length::Fill)
        .style(container::rounded_box)
}

#[path = "wabbajack_parts/panels.rs"]
mod panels;

use self::panels::{
    action_controls, format_size_summary, hm_controls, missing_archives_panel, progress_panel,
    readiness_panel, selected_entry, target_controls,
};
