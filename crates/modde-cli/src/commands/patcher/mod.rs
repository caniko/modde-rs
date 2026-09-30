//! Patcher command entrypoint and shared types.

mod fs_ops;
mod handlers;
mod pipeline;
mod process;

#[cfg(test)]
mod tests;

pub use handlers::{
    handle_add_command, handle_add_synthesis, handle_list, handle_remove, handle_reorder,
    handle_run, handle_run_stage, handle_set_enabled, handle_validate, run_enabled_for_deploy_at,
};

pub(crate) async fn validate_for_deploy(
    pm: &modde_core::profile::ProfileManager,
    profile: &modde_core::Profile,
    plugin: &dyn modde_games::GamePlugin,
) -> anyhow::Result<()> {
    if let Some(id) = profile.id {
        for stage in pm.db().list_patcher_stages(id).await?.iter().filter(|stage| stage.enabled) {
            pipeline::validate_stage_definition(stage)?;
            pipeline::validate_stage_runtime(stage, profile, plugin)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    sha256: String,
}
