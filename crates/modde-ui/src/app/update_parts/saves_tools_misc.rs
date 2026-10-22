#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! Saves, tools, executable, and misc update dispatch.

use super::*;

#[path = "saves_tools_misc_parts/executables_misc.rs"]
mod executables_misc;
#[path = "saves_tools_misc_parts/saves.rs"]
mod saves;
#[path = "saves_tools_misc_parts/tools.rs"]
mod tools;

impl Modde {
    pub(super) fn handle_saves_tools_misc_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadSaveHistory
            | Message::SelectSaveSnapshot(_)
            | Message::RestoreSaveSnapshot(_)
            | Message::DataTabFilterChanged(_)
            | Message::RunDiagnostics
            | Message::CrashLogPathChanged(_)
            | Message::AnalyzeCrashLog
            | Message::DiagnosticsComputed { .. } => self.handle_saves_update(message),

            Message::LoadTools
            | Message::RefreshTools
            | Message::LoadExecutables
            | Message::RefreshExecutables
            | Message::ToolsLoaded { .. }
            | Message::ToolSettingWritten { .. }
            | Message::ExecutablesLoaded { .. }
            | Message::RefreshOptiScalerReleases
            | Message::OptiScalerReleasesLoaded(_)
            | Message::InstallOptiScalerRelease
            | Message::OptiScalerReleaseInstalled(_)
            | Message::RefreshProtonVersions
            | Message::ProtonVersionsLoaded(_)
            | Message::InstallProtonVersion
            | Message::ProtonVersionInstalled(_)
            | Message::SelectToolTab(_)
            | Message::UpdateToolSetting { .. }
            | Message::ToggleTool { .. }
            | Message::ToggleToolAdvancedSettings
            | Message::ActivateOptiScaler
            | Message::DeactivateOptiScaler
            | Message::RestoreToolSettings { .. }
            | Message::ToolSettingsRestored { .. }
            | Message::ApplyTool(_)
            | Message::RevertTool(_)
            | Message::ToolApplied { .. }
            | Message::ToolReverted { .. } => self.handle_tools_update(message),

            _ => self.handle_executables_misc_update(message),
        }
    }
}
