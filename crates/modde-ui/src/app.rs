use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(test)]
use iced::Theme;
use iced::window;
use smallvec::SmallVec;

use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
#[cfg(test)]
use modde_core::resolver::GameId;
use modde_core::save::SaveSnapshot;
use modde_core::settings::AppSettings;

mod fomod_wizard_state;
mod install_ops;
mod model;
mod profile_ops;
mod state;
mod tool_ops;
mod tool_settings;
mod update;
mod view;

/// Drive an async database future to completion from a synchronous blocking
/// bridge.
///
/// modde's storage layer is async (`sqlx`), but some CPU/filesystem-heavy
/// loaders intentionally run in `tokio::task::spawn_blocking` because they
/// also perform synchronous CPU/filesystem work. Those blocking bodies still
/// need to call async DB APIs. This shim provides that bridge without starting
/// a nested Tokio runtime on iced's ambient executor. It also remains
/// acceptable for the one-time startup DB open before the first render.
///
/// Do not call this on the iced render/update/view path. If the work is pure DB
/// with owned inputs, make the loader `async` and `.await` the shared
/// `ModdeDb` handle inside `Task::perform`; if it also does CPU/filesystem work,
/// keep the whole synchronous body inside `spawn_blocking` and use this shim
/// there.
pub(crate) fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let rt = RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build modde-ui database runtime")
    });
    // Enter the shared runtime so sqlx (which needs the tokio reactor/timer)
    // has a handle, then drive the future to completion on *this* thread with
    // a current-thread executor. Driving on the current thread sidesteps the
    // "Cannot start a runtime from within a runtime" panic that `Runtime::block_on`
    // would hit on iced's ambient thread, and avoids requiring the future to be
    // `Send` (sqlx connection futures are not `Send` under a higher-ranked
    // bound), so it works for futures that borrow `&db`/`&self`.
    let _guard = rt.enter();
    futures::executor::block_on(future)
}

pub use self::fomod_wizard_state::FOMODWizardState;
pub(crate) use self::state::format_lock_reason;
pub use self::state::{
    AddCustomGameDraft, AddCustomGameDraftField, AddCustomGameState, ChecklistStatus,
    ConfigChecklistItem, DataTabConflicts, DiagnosticsComputed, ExecutableDraft,
    ExecutableDraftField, ExecutableUiEntry, ProfileContextSnapshot, ProfileLoadOutcome,
    ReorderDirection, SidebarGroup, ToolApplyResult, ToolHistoryUiEntry, ToolLoadSnapshot,
    ToolReleaseSupport, ToolRevertResult, ToolSettingWriteResult, ToolState, ToolUiEntry, View,
    WabbajackInstallerState, WabbajackTab,
};
pub use crate::views::library::{
    LibraryEntry, LibraryEntryKind, LibraryLoadResult, LibraryState, ManagerListInstance,
};
pub use self::tool_ops::parse_executable_environment;
#[cfg(test)]
use self::tool_ops::{apply_tool_for_game, validate_optiscaler_apply};
#[cfg(test)]
use self::tool_settings::{
    get_tool_setting_value, normalize_tool_settings_for_specs, set_nested_tool_setting,
    tool_apply_is_pending, tool_apply_signature,
};

pub type ToolOptionCatalog = HashMap<String, Vec<String>>;

const BUTTON_HOVER_TOAST_DELAY: Duration = Duration::from_secs(2);
// The delayed hover-toast lifecycle is still owned by Modde: app::update
// schedules Message::ButtonHoverElapsed after this delay.

mod helpers;
mod message;
mod runtime;
mod types;

use self::helpers::{
    build_conflict_rows, build_default_download_meta, detected_game_ids, format_diagnostic_entry,
    load_active_plugins_blocking, load_hidden_files_blocking, settings_game_install_paths,
};
pub use self::message::Message;
pub use self::runtime::run;
pub(crate) use self::runtime::shortcut_action_to_message;
use self::runtime::{external_refresh_stream, resize_thumbnail_bytes};
pub use self::types::{
    ButtonHoverToast, ButtonHoverToastState, ExperimentWriteKind, ExperimentWriteOutcome, Modde,
    NexusAuthStatus, ProfileWriteKind, ProfileWriteOutcome, SettingsGameInstall, SettingsState,
    WabbajackInstallEvent, WabbajackInstallUiSummary,
};

#[cfg(test)]
mod tests;
