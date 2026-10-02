#![allow(clippy::wildcard_imports)]
use super::*;

impl ButtonActionDescription for ButtonAction {
    fn button_description(&self) -> &'static str {
        match self {
            ButtonAction::SwitchView(_) => "Switch the main workspace to this section.",
            ButtonAction::ToggleSidebarGroup(_) => {
                "Expand or collapse this sidebar navigation group."
            }
            ButtonAction::DeleteProfile(_) => {
                "Delete the active profile and remove it from the profile list."
            }
            ButtonAction::OpenNewProfileDialog => {
                "Open a dialog for creating a new profile for the selected game."
            }
            ButtonAction::ForkProfile { .. } => {
                "Create a copy of the active profile that can be changed independently."
            }
            ButtonAction::RollbackExperiment => {
                "Discard the current profile experiment and return to the previous state."
            }
            ButtonAction::CommitExperiment => {
                "Keep the current experiment changes and make them the active profile state."
            }
            ButtonAction::TryProfile => {
                "Start an experimental profile layer so changes can be tested before committing."
            }
            ButtonAction::OpenModPage => "Open the selected mod's Nexus Mods page in a browser.",
            ButtonAction::ModGalleryNext => {
                "Show the next image from the selected mod's Nexus gallery."
            }
            ButtonAction::ModEndorseToggle => "Toggle your Nexus endorsement for the selected mod.",
            ButtonAction::ModTrackToggle => {
                "Toggle whether Nexus tracks updates for the selected mod."
            }
            ButtonAction::RestoreSaveSnapshot(_) => {
                "Restore this save snapshot and its captured save files."
            }
            ButtonAction::AddMod => "Choose a mod archive or folder to add to the active profile.",
            ButtonAction::RemoveMod(_) => "Remove the selected mod from the active profile.",
            ButtonAction::Deploy => "Deploy the active profile's enabled mods to the game folder.",
            ButtonAction::ToggleFilterMode => {
                "Switch whether mod list filters must all match or any one can match."
            }
            ButtonAction::CycleFilter(_) => {
                "Cycle this filter between ignored, required, and excluded."
            }
            ButtonAction::ClearFilters => "Clear all active mod list filters.",
            ButtonAction::ToggleCompactModList => {
                "Toggle between compact and normal spacing in the mod list."
            }
            ButtonAction::ToggleSeparator(_) => {
                "Expand or collapse this category group in the mod list."
            }
            ButtonAction::ReorderMod { .. } => {
                "Move this mod one position in the active profile load order."
            }
            ButtonAction::SelectMod(_) => "Select this mod and show its details in the sidebar.",
            ButtonAction::SearchCollections(_) => {
                "Run a Nexus Collections search using the current search text."
            }
            ButtonAction::InstallCollection { .. } => {
                "Start downloading and installing this Nexus collection."
            }
            ButtonAction::BrowseTabSwitched(_) => {
                "Switch the Nexus browser to this feed and load its results if needed."
            }
            ButtonAction::BrowseInstallMod { .. } => {
                "Install this Nexus mod into the active profile."
            }
            ButtonAction::LoadWabbajackCatalog => {
                "Refresh the Wabbajack catalog and authored file lists."
            }
            ButtonAction::WabbajackTabChanged(_) => "Switch the Wabbajack explorer to this tab.",
            ButtonAction::OpenWabbajackFile => "Choose a local .wabbajack file from disk.",
            ButtonAction::WabbajackDownloadSelected => {
                "Download the selected or entered Wabbajack modlist file."
            }
            ButtonAction::WabbajackCheckReadiness => {
                "Check whether the selected Wabbajack file is ready to install."
            }
            ButtonAction::WabbajackImportArchives => {
                "Import downloaded manual archives by exact Wabbajack hash."
            }
            ButtonAction::WabbajackStartInstall => {
                "Install the currently selected local Wabbajack file."
            }
            ButtonAction::WabbajackSelectEntry(_) => {
                "Select this Wabbajack entry and show its details."
            }
            ButtonAction::WabbajackOpenUrl(_) => "Open this Wabbajack page in your browser.",
            ButtonAction::WabbajackGenerateHmSnippet => {
                "Generate a Home Manager configuration snippet for this Wabbajack setup."
            }
            ButtonAction::WabbajackCopyHmSnippet => {
                "Copy the generated Home Manager snippet to the clipboard."
            }
            ButtonAction::WabbajackSaveHmSnippet => {
                "Save the generated Home Manager snippet to disk."
            }
            ButtonAction::FomodCancel => "Cancel the FOMOD installer and close the wizard.",
            ButtonAction::FomodUndo => "Undo the most recent FOMOD wizard selection change.",
            ButtonAction::FomodBack => "Return to the previous FOMOD installer step.",
            ButtonAction::FomodNext => {
                "Continue to the next FOMOD step or install when all required choices are ready."
            }
            ButtonAction::PauseDownload(_) => "Pause this active download.",
            ButtonAction::ResumeDownload(_) => "Resume or retry this download.",
            ButtonAction::CancelDownload(_) => "Cancel this queued or active download.",
            ButtonAction::RunDiagnostics => {
                "Scan the active profile for game-specific modding and integrity issues."
            }
            ButtonAction::AnalyzeCrashLog => {
                "Parse the selected crash log and correlate it with the active profile."
            }
            ButtonAction::ClearOverwrite => {
                "Delete all files currently stored in the profile override area."
            }
            ButtonAction::MoveOverwriteToMod(_) => {
                "Create a regular mod from the files currently in the override area."
            }
            ButtonAction::LoadSaveHistory => {
                "Refresh the list of captured save snapshots for the active profile."
            }
            ButtonAction::ValidateNexusKey => {
                "Validate the configured Nexus Mods API key and show account status."
            }
            ButtonAction::ToggleNexusApiKeyVisibility => {
                "Show or hide the Nexus Mods API key in the settings field."
            }
            ButtonAction::ReplaceNexusApiKey => {
                "Save this key to modde's own Nexus API key config file."
            }
            ButtonAction::RemoveNexusConfigKey => {
                "Remove only modde's own Nexus API key config file."
            }
            ButtonAction::BrowseGamePath => "Choose the game installation directory.",
            ButtonAction::BrowseDownloadDir => "Choose where downloaded mod archives are stored.",
            ButtonAction::CreateStockSnapshot => {
                "Capture a clean stock game snapshot for later deployment checks."
            }
            ButtonAction::VerifyStockSnapshot => {
                "Compare the current game installation with the saved stock snapshot."
            }
            ButtonAction::RefreshTools => {
                "Refresh detected gaming tools and overlay integration status."
            }
            ButtonAction::SelectToolTab(_) => "Switch to this tool's game-specific settings tab.",
            ButtonAction::UpdateToolSetting { .. } => {
                "Apply this value to the selected tool setting."
            }
            ButtonAction::ToggleToolAdvancedSettings => {
                "Show or hide advanced tool settings for the active tool."
            }
            ButtonAction::ApplyTool(_) => {
                "Apply this tool's required files or configuration to the game directory."
            }
            ButtonAction::RevertTool(_) => {
                "Remove this tool's applied files from the game directory."
            }
            ButtonAction::ActivateOptiScaler => {
                "Apply OptiScaler files and enable its launch integration."
            }
            ButtonAction::DeactivateOptiScaler => {
                "Revert OptiScaler files and disable its launch integration."
            }
            ButtonAction::AdoptOptiScaler => {
                "Record the detected OptiScaler files as managed for this game."
            }
            ButtonAction::RestoreOptiScalerBackup => {
                "Restore the latest backed-up OptiScaler files for this game."
            }
            ButtonAction::ResetOptiScalerConfig => {
                "Clear OptiScaler INI overrides so the selected release defaults are used."
            }
            ButtonAction::RestoreToolSettings { .. } => {
                "Restore this settings version without applying or reverting game files."
            }
            ButtonAction::RefreshOptiScalerReleases => {
                "Load OptiScaler release tags and assets from the official GitHub repository."
            }
            ButtonAction::InstallOptiScalerRelease => {
                "Download and cache the selected OptiScaler release for this game."
            }
            ButtonAction::RefreshProtonVersions => {
                "Load GE-Proton release versions from the official GitHub repository."
            }
            ButtonAction::InstallProtonVersion => {
                "Install the selected GEProton version through protonup-rs."
            }
            ButtonAction::OpenExecutableEditor => {
                "Open the editor for adding a new executable launch target."
            }
            ButtonAction::RefreshExecutables => {
                "Refresh executable launch targets for the selected game."
            }
            ButtonAction::ClearExecutableDraft => {
                "Close the executable editor and discard unsaved field values."
            }
            ButtonAction::EditExecutable(_) => {
                "Load this executable into the editor so its settings can be updated."
            }
            ButtonAction::SaveExecutable => {
                "Save the executable launch target for the selected game."
            }
            ButtonAction::RemoveExecutable(_) => {
                "Remove this executable launch target from the selected game."
            }
            ButtonAction::RunExecutable(_) => {
                "Run this executable through the active profile with overwrite capture."
            }
            ButtonAction::BrowseExecutablePath => "Choose the executable file to launch.",
            ButtonAction::BrowseExecutableWorkingDir => {
                "Choose the working directory for this executable."
            }
            ButtonAction::RefreshLibrary => {
                "Reload detected games and manager instances for the library."
            }
            ButtonAction::OpenLibraryLogs => "Open the private GUI and launch logs folder.",
            ButtonAction::ExportLibraryDiagnostics => {
                "Export the latest launch's redacted evidence; raw game output is excluded."
            }
            ButtonAction::FavoriteLibraryEntry(_) => "Save or remove this game as a favorite.",
            ButtonAction::InstallLibraryEntry(_) => {
                "Open this game's installation action in its store."
            }
            ButtonAction::SaveLibraryLaunch => {
                "Save the selected installation's launch configuration."
            }
            ButtonAction::EditLibraryLaunch => "Show or hide settings for this installation.",
            ButtonAction::BrowseLibraryLaunch(_) => {
                "Choose a file or folder for this launch setting."
            }
            ButtonAction::ImportLibraryLaunch => {
                "Load a launch settings JSON file into this draft for review."
            }
            ButtonAction::ExportLibraryLaunch => {
                "Export the current launch settings draft as JSON."
            }
            ButtonAction::InstallLibraryHook => {
                "Generate the command wrapper used by Steam or Heroic to launch this installation."
            }
            ButtonAction::AdoptLibrarySaves => {
                "Adopt existing saves into the saved profile without changing live files."
            }
            ButtonAction::FinishLibrarySession => {
                "Confirm the game has exited and capture this session's saves."
            }
            ButtonAction::SkipLibraryAnalysis => {
                "Keep captured saves and leave performance/bisect results for manual handling."
            }
            ButtonAction::SyncLibrarySteam => {
                "Refresh owned Steam games using MODDE_STEAM_API_KEY."
            }
            ButtonAction::SelectLibraryEntry(_) => {
                "Select this library entry and show its launch details."
            }
            ButtonAction::PlayLibraryEntry { hd, .. } => {
                if *hd {
                    "Launch this manager instance in HD mode."
                } else {
                    "Launch this game through its configured launcher."
                }
            }
            ButtonAction::ManageLibraryGame(_) => {
                "Select this game and open its mod list for management."
            }
            ButtonAction::WindowMinimize => "Minimize the modde window.",
            ButtonAction::WindowToggleMaximize => {
                "Toggle the modde window between maximized and restored size."
            }
            ButtonAction::WindowClose => "Close the modde window.",
            ButtonAction::CancelNewProfileDialog => {
                "Close the new profile dialog without creating a profile."
            }
            ButtonAction::SubmitNewProfileDialog => {
                "Create the profile using the entered name and selected game."
            }
            ButtonAction::GamePathDialogBrowse => {
                "Choose the selected game's installation directory."
            }
            ButtonAction::CancelGamePathDialog => {
                "Cancel setting the game path and return to the previous game selection."
            }
            ButtonAction::OpenAddCustomGame => "Open a dialog to register a new custom game.",
            ButtonAction::BrowseAddCustomGameInstallPath => {
                "Choose the custom game's install directory and scan it for executables."
            }
            ButtonAction::AddCustomGameSubmit => {
                "Save the custom game, reload the registry, and select it."
            }
            ButtonAction::AddCustomGameCancel => "Close the custom game dialog without saving.",
            ButtonAction::OpenManageCustomGames => {
                "Open the list of user-defined games and remove existing entries."
            }
            ButtonAction::CloseManageCustomGames => "Close the custom game manager.",
            ButtonAction::RemoveCustomGame(_) => {
                "Remove this user-defined game from the runtime registry."
            }
            ButtonAction::OpenUpdateReleasePage => {
                "Open the latest modde release page in your browser."
            }
            ButtonAction::DismissUpdateBanner => {
                "Hide this update notification for the current session."
            }
        }
    }
}
