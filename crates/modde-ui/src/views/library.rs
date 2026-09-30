use std::collections::HashSet;
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, pick_list, row, scrollable, text_input};
use iced::{Alignment, Element, Length, color};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;
use crate::views::selectable_text::text;
use modde_core::library::{LibraryPreferences, PendingSession};
use modde_games::library::LibraryGame;

mod settings;
pub use settings::{LaunchDraft, LaunchField};

/// Which backend owns a library entry. Game entries launch through the
/// detected Steam/Heroic launcher; manager entries launch through the
/// configured `modde-manager onboard launch` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryEntryKind {
    Game { game: LibraryGame },
    Manager { instance: String },
}

/// One selectable row in the game catalogue. Game and manager entries never
/// merge: distinct installations and same-named manager instances keep their
/// own ids so Play always targets exactly what the user selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub id: String,
    pub display_name: String,
    pub install_path: Option<PathBuf>,
    pub source_label: String,
    pub kind: LibraryEntryKind,
    pub launchable: bool,
    pub unavailability_reason: Option<String>,
    pub entitlement: String,
    pub favorite: bool,
    pub managed: bool,
}

impl LibraryEntry {
    #[must_use]
    pub fn game_id(&self) -> Option<&str> {
        match &self.kind {
            LibraryEntryKind::Game { game } => game.game_id.as_deref(),
            LibraryEntryKind::Manager { .. } => None,
        }
    }

    #[must_use]
    pub fn manager_instance(&self) -> Option<&str> {
        match &self.kind {
            LibraryEntryKind::Manager { instance } => Some(instance),
            LibraryEntryKind::Game { .. } => None,
        }
    }
}

/// Catalogue state: searchable entries plus the selected row. Launch work
/// tracks in-flight entry ids so duplicate clicks are ignored and late
/// results can be matched to the entry that started them.
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    pub entries: Vec<LibraryEntry>,
    pub filter: String,
    pub selected_id: Option<String>,
    pub loading: bool,
    pub load_error: Option<String>,
    pub launching: HashSet<String>,
    pub manager_config: Option<PathBuf>,
    pub manager_error: Option<String>,
    pub generation: u64,
    pub preferences: LibraryPreferences,
    pub notices: Vec<String>,
    pub category: LibraryFilter,
    pub draft: Option<LaunchDraft>,
    pub saving: bool,
    pub pending: Option<PendingSession>,
    pub steam_id: String,
    pub syncing: bool,
    pub editing: bool,
    pub(crate) mutation_dispatch: bool,
    pub(crate) refresh_ticks: u8,
    pub(crate) session_polling: bool,
    pub(crate) session_revision: u64,
    pub(crate) draft_revision: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LibraryFilter {
    #[default]
    All,
    Installed,
    Uninstalled,
    Favorites,
    Managed,
}

impl std::fmt::Display for LibraryFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::All => "All games",
            Self::Installed => "Installed",
            Self::Uninstalled => "Uninstalled",
            Self::Favorites => "Favorites",
            Self::Managed => "Modde-managed",
        })
    }
}

impl LibraryFilter {
    fn includes(self, entry: &LibraryEntry) -> bool {
        match self {
            Self::All => true,
            Self::Installed => entry.install_path.is_some(),
            Self::Uninstalled => entry.install_path.is_none(),
            Self::Favorites => entry.favorite,
            Self::Managed => entry.managed,
        }
    }
}

impl LibraryState {
    pub(crate) fn replace_draft(&mut self, draft: Option<LaunchDraft>) {
        self.draft_revision = self.draft_revision.wrapping_add(1);
        self.draft = draft;
    }

    pub(crate) fn edit_draft(&mut self, edit: impl FnOnce(&mut LaunchDraft)) {
        if self.saving || !self.launching.is_empty() || self.pending.is_some() {
            return;
        }
        if let Some(draft) = &mut self.draft {
            edit(draft);
            self.draft_revision = self.draft_revision.wrapping_add(1);
        }
    }

    pub(crate) fn draft_matches(&self, id: &str, revision: u64) -> bool {
        revision == self.draft_revision
            && self.selected_id.as_deref() == Some(id)
            && self.draft.as_ref().is_some_and(|draft| draft.id == id)
    }

    pub fn launch_settings_dirty(&self, id: &str) -> bool {
        self.draft
            .as_ref()
            .filter(|draft| draft.id == id)
            .is_some_and(|draft| {
                draft.settings().ok().as_ref() != Some(&self.preferences.launch_for(id))
            })
    }

    #[must_use]
    pub fn selected_entry(&self) -> Option<&LibraryEntry> {
        self.selected_id
            .as_ref()
            .and_then(|id| self.entries.iter().find(|entry| &entry.id == id))
    }

    #[must_use]
    pub fn is_launching(&self, id: &str) -> bool {
        self.launching.contains(id)
    }
}

/// Async library load result: game entries always load; manager entries fold
/// in when the manager binary/config resolve, otherwise `manager_error`
/// carries the reason and the games still display.
#[derive(Debug, Clone)]
pub struct LibraryLoadResult {
    pub entries: Vec<LibraryEntry>,
    pub manager_config: Option<PathBuf>,
    pub manager_error: Option<String>,
    pub preferences: LibraryPreferences,
    pub notices: Vec<String>,
    pub pending: Option<PendingSession>,
}

/// One parsed `modde-manager list --json` instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerListInstance {
    pub name: String,
    pub root: PathBuf,
    pub client: String,
}

/// Parse `modde-manager list --json` output (`{"instances": [...]}`).
pub fn parse_manager_list_json(output: &str) -> Result<Vec<ManagerListInstance>, String> {
    let value: serde_json::Value =
        serde_json::from_str(output).map_err(|err| format!("invalid manager list JSON: {err}"))?;
    let instances = value
        .get("instances")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "manager list JSON has no 'instances' array".to_string())?;
    let mut parsed = Vec::with_capacity(instances.len());
    for instance in instances {
        let name = instance
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "manager instance is missing 'name'".to_string())?;
        let root = instance
            .get("root")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("manager instance '{name}' is missing 'root'"))?;
        let client = instance
            .get("client")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        parsed.push(ManagerListInstance {
            name: name.to_string(),
            root: PathBuf::from(root),
            client: client.to_string(),
        });
    }
    Ok(parsed)
}

/// Stable entry id for a detected/configured game installation. The path is
/// part of the id so two installs of the same game stay distinct.
#[must_use]
pub fn game_entry_id(game_id: &str, path: &Path) -> String {
    modde_core::library::installation_id(&format!("local:{game_id}"), path)
}

/// Stable entry id for a manager instance.
#[must_use]
pub fn manager_entry_id(instance: &str) -> String {
    format!("manager:{instance}")
}

/// Minimal detected-game shape so the view stays testable without a launcher
/// scan. Callers map `modde_games::DetectedGame` plus configured paths into
/// this before building entries.
#[derive(Debug, Clone)]
pub struct LibraryGameInstall {
    pub game_id: String,
    pub display_name: String,
    pub install_path: PathBuf,
    pub source_label: String,
}

/// Build configured installations. A directory alone is not a launch command.
#[must_use]
pub fn build_game_entries(games: Vec<LibraryGameInstall>) -> Vec<LibraryEntry> {
    games
        .into_iter()
        .map(|game| {
            let mut model = LibraryGame::new(
                modde_games::library::Store::Local,
                game.game_id.clone(),
                game.display_name.clone(),
                Some(game.install_path.clone()),
            );
            model.game_id = Some(game.game_id.clone());
            let id = model.id.clone();
            LibraryEntry {
                id,
                display_name: game.display_name,
                install_path: Some(game.install_path),
                source_label: game.source_label,
                kind: LibraryEntryKind::Game {
                    game: model.clone(),
                },
                launchable: false,
                unavailability_reason: Some("Choose an executable in Launch Settings".into()),
                entitlement: model.entitlement,
                favorite: false,
                managed: false,
            }
        })
        .collect()
}

pub fn build_catalogue_entries(
    games: &[LibraryGame],
    preferences: &LibraryPreferences,
    managed_games: &HashSet<String>,
) -> Vec<LibraryEntry> {
    games
        .iter()
        .map(|game| {
            let settings = preferences.launch_for(&game.id);
            let readiness = modde_games::library::launch::validate(game, &settings, games);
            LibraryEntry {
                id: game.id.clone(),
                display_name: game.name.clone(),
                install_path: game.install_path.clone(),
                source_label: game.store.label().to_string(),
                kind: LibraryEntryKind::Game { game: game.clone() },
                launchable: readiness.is_ok(),
                unavailability_reason: readiness.err().map(|error| error.to_string()),
                entitlement: game.entitlement.clone(),
                favorite: preferences.favorites.contains(&game.entitlement),
                managed: settings.profile.is_some()
                    || game
                        .game_id
                        .as_ref()
                        .is_some_and(|id| managed_games.contains(id)),
            }
        })
        .collect()
}

/// Build one entry per manager instance, launched via `onboard launch`.
#[must_use]
pub fn build_manager_entries(instances: Vec<ManagerListInstance>) -> Vec<LibraryEntry> {
    instances
        .into_iter()
        .map(|instance| {
            let label = if instance.client.is_empty() {
                instance.name.clone()
            } else {
                format!("{} ({})", instance.name, instance.client)
            };
            LibraryEntry {
                id: manager_entry_id(&instance.name),
                display_name: label,
                install_path: Some(instance.root),
                source_label: "Manager".to_string(),
                kind: LibraryEntryKind::Manager {
                    instance: instance.name.clone(),
                },
                launchable: true,
                unavailability_reason: None,
                entitlement: manager_entry_id(&instance.name),
                favorite: false,
                managed: true,
            }
        })
        .collect()
}

/// Favorites first, then modde-managed games, then name and identity. Same
/// display names never collapse: each entry keeps its own backend id.
#[must_use]
pub fn merge_library_entries(
    mut games: Vec<LibraryEntry>,
    mut managers: Vec<LibraryEntry>,
) -> Vec<LibraryEntry> {
    games.append(&mut managers);
    games.sort_by(|a, b| {
        (!a.favorite)
            .cmp(&!b.favorite)
            .then_with(|| (!a.managed).cmp(&!b.managed))
            .then_with(|| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    games
}

/// Case-insensitive filter over name, source, and path.
#[must_use]
pub fn filter_library_entries<'a>(
    entries: &'a [LibraryEntry],
    filter: &str,
) -> Vec<&'a LibraryEntry> {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return entries.iter().collect();
    }
    entries
        .iter()
        .filter(|entry| {
            entry.display_name.to_lowercase().contains(&needle)
                || entry.source_label.to_lowercase().contains(&needle)
                || entry
                    .install_path
                    .as_ref()
                    .is_some_and(|path| path.display().to_string().to_lowercase().contains(&needle))
        })
        .collect()
}

/// Render the Library catalogue: filter + refresh on top, entry list on the
/// left, selected entry details (Play / Manage Mods) on the right.
pub fn view(state: &LibraryState) -> Element<'_, Message> {
    let count_label = match state.entries.len() {
        1 => "1 in library".to_string(),
        count => format!("{count} in library"),
    };
    let title_bar = row![
        text("Library").size(20),
        text(count_label).size(11).color(color!(0x888888)),
        iced::widget::space::horizontal(),
        pick_list(
            [
                LibraryFilter::All,
                LibraryFilter::Installed,
                LibraryFilter::Uninstalled,
                LibraryFilter::Favorites,
                LibraryFilter::Managed
            ],
            Some(state.category),
            Message::LibraryCategoryChanged
        ),
        text_input("Filter games...", &state.filter)
            .on_input(Message::LibraryFilterChanged)
            .width(Length::Fixed(220.0)),
        crate::semantics::test_id(
            "library.refresh",
            button(text("Refresh").size(12))
                .style(button::secondary)
                .padding([5, 12])
                .on_action_maybe(
                    (!state.loading).then_some(ButtonAction::RefreshLibrary),
                    "The library is already loading.",
                ),
        ),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let mut content = column![title_bar].spacing(10);
    content = content.push(
        row![
            text("Steam account").size(12),
            text_input("SteamID64", &state.steam_id)
                .on_input(Message::LibrarySteamIdChanged)
                .width(Length::Fixed(190.0)),
            button(
                text(if state.syncing {
                    "Syncing..."
                } else {
                    "Sync owned games"
                })
                .size(12)
            )
            .on_action_maybe(
                (!state.syncing).then_some(ButtonAction::SyncLibrarySteam),
                "Steam sync is in progress."
            ),
            text("API key: MODDE_STEAM_API_KEY").size(11),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    for notice in &state.notices {
        content = content.push(text(notice).size(11));
    }
    if let Some(session) = &state.pending {
        let preparing = session.phase.is_preparation();
        let awaiting = session.phase == modde_core::library::SessionPhase::AwaitingStore;
        let captured = session.phase == modde_core::library::SessionPhase::Captured;
        content = content.push(row![
            text(format!("{} — {}. {}", session.name, session.phase.label(),
                if awaiting { "The store will invoke the installed command wrapper." }
                else if preparing { "Interrupted operations can be recovered before retrying." }
                else if captured { "Saves are captured. Retry analysis or leave its result for manual handling." }
                else if session.observation.is_some() { "Observed exit triggers automatic completion." }
                else { "Confirm completion after the store-launched game has exited." })).size(12),
            button(text(if awaiting { "Cancel pending request" } else if preparing { "Recover operation" } else if captured { "Retry analysis" } else { "Confirm exited & capture" }).size(12)).on_action_maybe(
                state.launching.is_empty().then_some(ButtonAction::FinishLibrarySession), "Wait for the current launch task."),
        ].spacing(8));
        if captured {
            content = content.push(
                button(text("Skip automatic analysis").size(12)).on_action_maybe(
                    state
                        .launching
                        .is_empty()
                        .then_some(ButtonAction::SkipLibraryAnalysis),
                    "Wait for the current completion task.",
                ),
            );
        }
    }
    if state.loading {
        content = content.push(text("Loading library...").size(12).color(color!(0xAAAAAA)));
    }
    if let Some(error) = &state.load_error {
        content = content.push(
            text(format!("Failed to load library: {error}"))
                .size(12)
                .color(color!(0xFF8888)),
        );
    }
    if let Some(error) = &state.manager_error {
        content = content.push(
            text(format!("Manager: {error}"))
                .size(12)
                .color(color!(0xFFAA44)),
        );
    }

    let visible: Vec<_> = filter_library_entries(&state.entries, &state.filter)
        .into_iter()
        .filter(|entry| state.category.includes(entry))
        .collect();
    let selected = state.selected_entry();
    if visible.is_empty() {
        content = content.push(
            text(if state.entries.is_empty() {
                "No games found. Detect a game or configure a manager instance."
            } else {
                "No games match this filter."
            })
            .size(12)
            .color(color!(0xAAAAAA)),
        );
    } else {
        let mut list = column![].spacing(4);
        for entry in visible {
            let is_selected = state.selected_id.as_deref() == Some(entry.id.as_str());
            let label = format!(
                "{}{} — {} · {}{}",
                if entry.favorite { "★ " } else { "" },
                entry.display_name,
                entry.source_label,
                if entry.install_path.is_some() {
                    "Installed"
                } else {
                    "Uninstalled"
                },
                if entry.managed { " · Modde" } else { "" }
            );
            let row_button = if is_selected {
                button(text(label).size(13))
                    .style(button::primary)
                    .width(Length::Fill)
                    .padding([6, 10])
                    .described_disabled("This game is already selected.")
            } else {
                button(text(label).size(13))
                    .style(button::secondary)
                    .width(Length::Fill)
                    .padding([6, 10])
                    .on_action(ButtonAction::SelectLibraryEntry(entry.id.clone()))
            };
            list = list.push(row_button);
        }
        let details: Element<Message> = selected.map_or_else(
            || {
                container(
                    text("Select a game to see launch details.")
                        .size(12)
                        .color(color!(0xAAAAAA)),
                )
                .padding(12)
                .width(Length::Fill)
                .style(container::rounded_box)
                .into()
            },
            |entry| entry_details(entry, state),
        );
        content = content.push(
            row![
                scrollable(list).width(Length::FillPortion(3)),
                scrollable(details).width(Length::FillPortion(2))
            ]
            .spacing(12)
            .height(Length::Fill),
        );
    }

    container(content)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn entry_details<'a>(entry: &'a LibraryEntry, state: &'a LibraryState) -> Element<'a, Message> {
    let launching = !state.launching.is_empty() || state.pending.is_some();
    let dirty = state.launch_settings_dirty(&entry.id);
    let mut panel = column![text(entry.display_name.as_str()).size(16)].spacing(6);
    panel = panel.push(
        button(
            text(if entry.favorite {
                "Remove favorite"
            } else {
                "Add favorite"
            })
            .size(12),
        )
        .on_action_maybe(
            (!state.saving).then_some(ButtonAction::FavoriteLibraryEntry(entry.id.clone())),
            "Saving preferences.",
        ),
    );
    panel = panel.push(
        text(format!("Source: {}", entry.source_label))
            .size(12)
            .color(color!(0xAAAAAA)),
    );
    if let Some(path) = &entry.install_path {
        panel = panel.push(
            text(path.display().to_string())
                .size(11)
                .color(color!(0x888888)),
        );
    }
    if entry.install_path.is_none() {
        panel = panel.push(
            button(text(format!("Install via {}", entry.source_label)).size(12)).on_action_maybe(
                (!launching).then_some(ButtonAction::InstallLibraryEntry(entry.id.clone())),
                "Finish the current session first.",
            ),
        );
        return panel.padding(12).width(Length::Fill).into();
    }
    {
        panel = panel.push(
            button(
                text(if state.editing {
                    "Hide launch settings"
                } else {
                    "Launch settings"
                })
                .size(12),
            )
            .on_action(ButtonAction::EditLibraryLaunch),
        );
        if state.editing
            && let Some(draft) = &state.draft
        {
            panel = panel.push(settings::view(draft, state.saving || launching, entry));
        }
        if dirty {
            panel = panel.push(text("Unsaved launch settings — save before Play.").size(12));
        }
    }
    if !entry.launchable {
        panel = panel.push(
            text(
                entry
                    .unavailability_reason
                    .as_deref()
                    .unwrap_or("Launch not configured"),
            )
            .size(12)
            .color(color!(0xFF8888)),
        );
        if let Some(game_id) = entry.game_id() {
            panel = panel.push(button(text("Manage Mods").size(12)).on_action_maybe(
                (!launching).then_some(ButtonAction::ManageLibraryGame(game_id.to_string())),
                "Finish the current session first.",
            ));
        }
        return panel.padding(12).width(Length::Fill).into();
    }
    let play_label = if launching { "Session active" } else { "Play" };
    let busy_help = "This game is already launching.";
    match &entry.kind {
        LibraryEntryKind::Game { .. } => {
            let mut actions = row![crate::semantics::test_id(
                "library.play",
                button(text(play_label).size(12))
                    .style(button::success)
                    .padding([5, 14])
                    .on_action_maybe(
                        (!launching && !state.saving && !dirty).then_some(
                            ButtonAction::PlayLibraryEntry {
                                id: entry.id.clone(),
                                hd: false,
                            }
                        ),
                        busy_help,
                    ),
            ),]
            .spacing(8);
            if let Some(game_id) = entry.game_id() {
                actions = actions.push(crate::semantics::test_id(
                    "library.manage",
                    button(text("Manage Mods").size(12))
                        .style(button::secondary)
                        .padding([5, 12])
                        .on_action_maybe(
                            (!launching)
                                .then_some(ButtonAction::ManageLibraryGame(game_id.to_string())),
                            "Finish the current session first.",
                        ),
                ));
            }
            panel.push(actions)
        }
        LibraryEntryKind::Manager { .. } => panel.push(
            row![
                crate::semantics::test_id(
                    "library.play",
                    button(text(play_label).size(12))
                        .style(button::success)
                        .padding([5, 14])
                        .on_action_maybe(
                            (!launching && !state.saving && !dirty).then_some(
                                ButtonAction::PlayLibraryEntry {
                                    id: entry.id.clone(),
                                    hd: false,
                                }
                            ),
                            busy_help,
                        ),
                ),
                button(text(if launching { "Launching HD" } else { "Play HD" }).size(12))
                    .style(button::secondary)
                    .padding([5, 12])
                    .on_action_maybe(
                        (!launching && !state.saving && !dirty).then_some(
                            ButtonAction::PlayLibraryEntry {
                                id: entry.id.clone(),
                                hd: true,
                            }
                        ),
                        busy_help,
                    ),
            ]
            .spacing(8),
        ),
    }
    .padding(12)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(game_id: &str, name: &str, path: &str, source: &str) -> LibraryGameInstall {
        LibraryGameInstall {
            game_id: game_id.to_string(),
            display_name: name.to_string(),
            install_path: PathBuf::from(path),
            source_label: source.to_string(),
        }
    }

    #[test]
    fn distinct_installs_keep_distinct_ids() {
        let entries = build_game_entries(vec![
            game("skyrim-se", "Skyrim", "/games/a", "Steam (1)"),
            game("skyrim-se", "Skyrim", "/games/b", "Heroic/GOG (2)"),
        ]);

        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0].id, entries[1].id);
        assert!(entries.iter().all(|entry| !entry.launchable));
    }

    #[test]
    fn same_display_name_never_merges_backends() {
        let games = build_game_entries(vec![game("wow", "Wow", "/games/wow", "Steam (1)")]);
        let managers = build_manager_entries(vec![ManagerListInstance {
            name: "Wow".to_string(),
            root: PathBuf::from("/manager/wow"),
            client: "wow-wotlk".to_string(),
        }]);

        let merged = merge_library_entries(games, managers);

        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|entry| entry.game_id().is_some()));
        assert!(
            merged
                .iter()
                .any(|entry| entry.manager_instance().is_some())
        );
    }

    #[test]
    fn filter_matches_name_source_and_path() {
        let entries = merge_library_entries(
            build_game_entries(vec![
                game("skyrim-se", "Skyrim", "/games/skyrim", "Steam (1)"),
                game("fallout4", "Fallout 4", "/games/fallout", "Heroic/GOG (2)"),
            ]),
            Vec::new(),
        );

        assert_eq!(filter_library_entries(&entries, "sky").len(), 1);
        assert_eq!(filter_library_entries(&entries, "heroic").len(), 1);
        assert_eq!(filter_library_entries(&entries, "/games/fallout").len(), 1);
        assert_eq!(filter_library_entries(&entries, "nomatch").len(), 0);
    }

    #[test]
    fn parse_manager_list_json_reads_instances() {
        let instances = parse_manager_list_json(
            r#"{"instances": [{"name": "b", "root": "/b", "client": "wow-wotlk"}]}"#,
        )
        .expect("parse");

        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "b");
        assert_eq!(instances[0].root, PathBuf::from("/b"));
    }

    #[test]
    fn parse_manager_list_json_rejects_missing_array() {
        let err = parse_manager_list_json(r#"{"nope": []}"#).unwrap_err();
        assert!(err.contains("instances"));
    }

    #[test]
    fn favorites_sort_before_managed_and_other_games() {
        let mut games = build_game_entries(vec![
            game("other", "A other", "/other", "Steam"),
            game("favorite", "Z favorite", "/favorite", "Steam"),
            game("managed", "B managed", "/managed", "Steam"),
        ]);
        games[1].favorite = true;
        games[2].managed = true;
        let entries = merge_library_entries(games, Vec::new());
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.display_name.as_str())
                .collect::<Vec<_>>(),
            ["Z favorite", "B managed", "A other"]
        );
        assert!(LibraryFilter::Favorites.includes(&entries[0]));
        assert!(!LibraryFilter::Managed.includes(&entries[0]));
        assert!(LibraryFilter::Managed.includes(&entries[1]));
    }

    #[test]
    fn owned_uninstalled_game_is_filterable_but_not_playable() {
        let game = LibraryGame::new(
            modde_games::library::Store::Steam,
            "987654321".into(),
            "Owned title".into(),
            None,
        );
        let entries =
            build_catalogue_entries(&[game], &LibraryPreferences::default(), &HashSet::new());
        assert_eq!(filter_library_entries(&entries, "owned").len(), 1);
        assert!(!entries[0].launchable);
        assert!(LibraryFilter::Uninstalled.includes(&entries[0]));
        assert!(!LibraryFilter::Installed.includes(&entries[0]));
    }
}
