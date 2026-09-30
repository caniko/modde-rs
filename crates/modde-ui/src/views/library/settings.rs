use std::path::PathBuf;

use iced::widget::{checkbox, column, pick_list, text_input};
use iced::{Element, Length};
use modde_core::library::LaunchSettings;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;
use crate::views::selectable_text::text;

#[derive(Debug, Clone, Copy)]
pub enum LaunchField { Executable, Runner, Prefix, Arguments, Wrappers, Environment, WorkingDirectory, Profile, Saves, ReadOnly, Writable }

#[derive(Debug, Clone)]
pub struct LaunchDraft {
    pub id: String,
    base: LaunchSettings,
    executable: String,
    runner: String,
    prefix: String,
    arguments: String,
    wrappers: String,
    environment: String,
    working_directory: String,
    profile: String,
    saves: String,
    read_only: String,
    writable: String,
    pub sandbox: bool,
    pub network: bool,
    pub use_active_profile: bool,
    pub store_hook: bool,
    pub profiles: Vec<String>,
}

impl LaunchDraft {
    pub fn new(id: String, settings: LaunchSettings) -> Self {
        let path = |value: &Option<PathBuf>| value.as_ref().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
        Self {
            id, executable: path(&settings.executable), runner: path(&settings.runner), prefix: path(&settings.prefix),
            arguments: serde_json::to_string(&settings.arguments).unwrap_or_default(),
            wrappers: serde_json::to_string(&settings.wrappers).unwrap_or_default(),
            environment: serde_json::to_string(&settings.environment).unwrap_or_default(),
            working_directory: path(&settings.working_directory), profile: settings.profile.clone().unwrap_or_default(),
            saves: path(&settings.save_directory), read_only: serde_json::to_string(&settings.sandbox.read_only).unwrap_or_default(),
            writable: serde_json::to_string(&settings.sandbox.writable).unwrap_or_default(),
            sandbox: settings.sandbox.enabled, network: settings.sandbox.network,
            use_active_profile: settings.use_active_profile, store_hook: settings.store_hook, profiles: Vec::new(), base: settings,
        }
    }

    pub fn set(&mut self, field: LaunchField, value: String) {
        *match field {
            LaunchField::Executable => &mut self.executable, LaunchField::Runner => &mut self.runner,
            LaunchField::Prefix => &mut self.prefix, LaunchField::Arguments => &mut self.arguments,
            LaunchField::Wrappers => &mut self.wrappers,
            LaunchField::Environment => &mut self.environment, LaunchField::WorkingDirectory => &mut self.working_directory,
            LaunchField::Profile => &mut self.profile, LaunchField::Saves => &mut self.saves,
            LaunchField::ReadOnly => &mut self.read_only, LaunchField::Writable => &mut self.writable,
        } = value;
    }

    pub fn settings(&self) -> Result<LaunchSettings, String> {
        let path = |value: &str| (!value.trim().is_empty()).then(|| PathBuf::from(value));
        let mut settings = self.base.clone();
        settings.executable = path(&self.executable);
        settings.runner = path(&self.runner);
        settings.prefix = path(&self.prefix);
        settings.working_directory = path(&self.working_directory);
        settings.save_directory = path(&self.saves);
        settings.profile = (!self.profile.trim().is_empty()).then(|| self.profile.trim().to_string());
        settings.use_active_profile = self.use_active_profile;
        settings.store_hook = self.store_hook;
        settings.wrappers = serde_json::from_str(&self.wrappers).map_err(|error| format!("Wrappers must be an array of argument arrays: {error}"))?;
        settings.arguments = serde_json::from_str(&self.arguments).map_err(|error| format!("Arguments must be a JSON string array: {error}"))?;
        settings.environment = serde_json::from_str(&self.environment).map_err(|error| format!("Environment must be a JSON string map: {error}"))?;
        settings.sandbox.read_only = serde_json::from_str(&self.read_only).map_err(|error| format!("Read-only mounts must be a JSON path array: {error}"))?;
        settings.sandbox.writable = serde_json::from_str(&self.writable).map_err(|error| format!("Writable mounts must be a JSON path array: {error}"))?;
        settings.sandbox.enabled = self.sandbox;
        settings.sandbox.network = self.network;
        Ok(settings)
    }
}

pub(super) fn view<'a>(draft: &'a LaunchDraft, busy: bool, entry: &super::LibraryEntry) -> Element<'a, Message> {
    let manager = entry.manager_instance().is_some();
    let store = matches!(&entry.kind, super::LibraryEntryKind::Game { game } if game.store != modde_games::library::Store::Local);
    let saves = entry.game_id().and_then(modde_games::resolve_game_plugin).is_some_and(|plugin| plugin.supports_save_profiles());
    let mut fields = column![text("Launch Settings").size(14),
        text(if manager { "Manager supplies the validated executable, runner and prefix." }
            else if store { "Leave executable empty to use the store's command wrapper or saved launch options." }
            else { "Choose the executable for this installation." }).size(11),
    ].spacing(6);
    for (label, placeholder, value, field) in [
        ("Executable", "/absolute/path/to/game", &draft.executable, LaunchField::Executable),
        ("Runner (optional)", "/absolute/path/to/wine or umu-run", &draft.runner, LaunchField::Runner),
        ("Wine prefix (optional)", "/absolute/path/to/prefix", &draft.prefix, LaunchField::Prefix),
        ("Arguments (JSON array)", "[\"--flag\", \"value with spaces\"]", &draft.arguments, LaunchField::Arguments),
        ("Wrappers (outermost first)", "[[\"gamescope\", \"--\"]]", &draft.wrappers, LaunchField::Wrappers),
        ("Environment (JSON object)", "{\"KEY\":\"value\"}", &draft.environment, LaunchField::Environment),
        ("Working directory (optional)", "Defaults to installation directory", &draft.working_directory, LaunchField::WorkingDirectory),
        ("Mod profile (optional)", "Saved profile name", &draft.profile, LaunchField::Profile),
        ("Save directory", "/absolute/path/to/saves", &draft.saves, LaunchField::Saves),
        ("Read-only mounts (JSON array)", "[]", &draft.read_only, LaunchField::ReadOnly),
        ("Writable mounts (JSON array)", "[]", &draft.writable, LaunchField::Writable),
    ] {
        if manager && matches!(field, LaunchField::Executable | LaunchField::Runner | LaunchField::Prefix | LaunchField::Profile | LaunchField::Saves) { continue; }
        fields = fields.push(text(label).size(11)).push(text_input(placeholder, value)
            .on_input_maybe((!busy).then_some(move |value| Message::LibraryLaunchFieldChanged(field, value))));
        if matches!(field, LaunchField::Executable | LaunchField::Runner | LaunchField::Prefix | LaunchField::WorkingDirectory | LaunchField::Saves) {
            fields = fields.push(iced::widget::button(text(format!("Browse {label}")).size(11))
                .on_action_maybe((!busy).then_some(ButtonAction::BrowseLibraryLaunch(field)), "A session is in progress."));
        }
    }
    if !manager {
    if !draft.profiles.is_empty() {
        fields = fields.push(text("Saved mod profiles").size(11));
        if !busy {
            fields = fields.push(pick_list(draft.profiles.as_slice(), draft.profiles.iter().find(|name| **name == draft.profile).cloned(),
                |name| Message::LibraryLaunchFieldChanged(LaunchField::Profile, name)).placeholder("Select a profile"));
        }
    }
    fields = fields.push(checkbox(draft.use_active_profile).label("Use the active profile when the profile name is blank")
        .on_toggle_maybe((!busy).then_some(Message::LibraryActiveProfileChanged)));
    fields = fields.push(text("Blank profile + unchecked: leave deployed mods and live saves as they are; skip profile management.").size(11));
    if store {
    fields = fields.push(checkbox(draft.store_hook).label("Use the installed Steam / Heroic command wrapper")
        .on_toggle_maybe((!busy).then_some(Message::LibraryHookChanged)));
    fields = fields.push(iced::widget::button(text("Set up store wrapper").size(12))
        .on_action_maybe((!busy).then_some(ButtonAction::InstallLibraryHook), "A session is in progress."));
    }
    }
    fields = fields.push(checkbox(draft.sandbox).label("Sandbox this game with bubblewrap (Linux)")
        .on_toggle_maybe((!busy).then_some(Message::LibrarySandboxChanged)));
    fields = fields.push(checkbox(draft.network).label("Allow network in sandbox")
        .on_toggle_maybe((!busy).then_some(Message::LibraryNetworkChanged)));
    fields = fields.push(text("Sandboxing applies to the actual game command. GPU, display, audio and controllers are shared; extra files need explicit mounts.").size(11));
    fields = fields.push(iced::widget::row![
        iced::widget::button(text("Import JSON").size(12)).on_action_maybe((!busy).then_some(ButtonAction::ImportLibraryLaunch), "A session is in progress."),
        iced::widget::button(text("Export JSON").size(12)).on_action(ButtonAction::ExportLibraryLaunch),
    ].spacing(8));
    let mut actions = iced::widget::row![
        iced::widget::button(text("Save launch settings").size(12))
            .on_action_maybe((!busy).then_some(ButtonAction::SaveLibraryLaunch), "A session or settings save is in progress."),
    ].spacing(8);
    if saves {
        actions = actions.push(iced::widget::button(text("Adopt existing saves").size(12))
            .on_action_maybe((!busy).then_some(ButtonAction::AdoptLibrarySaves), "A session or settings save is in progress."));
    }
    fields.push(actions).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_keeps_argument_boundaries_and_hidden_wrapper_settings() {
        let saved = LaunchSettings {
            arguments: vec!["a b".into(), "$(not-a-shell)".into()],
            wrappers: vec![vec!["gamescope".into(), "--".into()]],
            ..LaunchSettings::default()
        };
        let mut draft = LaunchDraft::new("install".into(), saved.clone());
        assert_eq!(draft.settings().unwrap(), saved);
        draft.set(LaunchField::Arguments, "--invalid-json".into());
        assert!(draft.settings().is_err());
    }
}
