use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::crash::CrashCorrelationInput;
use modde_core::doctor::{
    DoctorContext, DoctorContextInput, DoctorLlmProvider, DoctorLlmRequest,
    DoctorProfileModSnapshot, build_doctor_context, diff_profile_snapshots,
    explain_with_openai_compatible_chat,
};
use modde_core::paths;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::resolver::GameId;
use modde_core::{CollisionReport, Diagnostic, PluginEntry};

use super::crash::CrashFormatArg;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum DoctorProviderArg {
    Local,
    Remote,
}

impl From<DoctorProviderArg> for DoctorLlmProvider {
    fn from(value: DoctorProviderArg) -> Self {
        match value {
            DoctorProviderArg::Local => Self::Local,
            DoctorProviderArg::Remote => Self::Remote,
        }
    }
}

pub async fn handle_profile(game: String, profile_name: Option<String>, json: bool) -> Result<()> {
    let bundle = load_bundle(&game, profile_name, None, CrashFormatArg::Auto).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&bundle.context)?);
    } else {
        print_profile_report(&bundle);
    }
    Ok(())
}

pub async fn handle_crash(
    log_path: Option<PathBuf>,
    game: String,
    profile_name: Option<String>,
    format: CrashFormatArg,
    json: bool,
) -> Result<()> {
    let log_path = match log_path {
        Some(path) => path,
        None => super::crash::discover_latest_crash_log(&game)?,
    };
    let bundle = load_bundle(&game, profile_name, Some(log_path), format).await?;
    bundle.record_crash_if_present().await?;
    report_compatibility_crash(&bundle).await;
    if json {
        println!("{}", serde_json::to_string_pretty(&bundle.context)?);
    } else {
        print_crash_report(&bundle.context);
    }
    Ok(())
}

pub async fn handle_explain(
    log_path: PathBuf,
    game: String,
    profile_name: Option<String>,
    provider: DoctorProviderArg,
    json: bool,
) -> Result<()> {
    let bundle = load_bundle(&game, profile_name, Some(log_path), CrashFormatArg::Auto).await?;
    bundle.record_crash_if_present().await?;
    report_compatibility_crash(&bundle).await;
    let settings = modde_core::settings::AppSettings::load();
    let explanation = explain_with_openai_compatible_chat(
        &settings.doctor.llm,
        DoctorLlmRequest {
            provider: provider.into(),
            context: bundle.context.clone(),
        },
    )
    .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&explanation)?);
    } else {
        print_explanation(&explanation);
    }
    Ok(())
}

#[cfg(feature = "remote-telemetry")]
async fn report_compatibility_crash(bundle: &DoctorBundle) {
    let Some(crash) = &bundle.context.crash else {
        return;
    };
    if let Err(error) = crate::telemetry::try_report_compatibility_crash(crash).await {
        tracing::warn!(%error, "failed to report compatibility crash telemetry");
    }
}

#[cfg(not(feature = "remote-telemetry"))]
async fn report_compatibility_crash(_bundle: &DoctorBundle) {}

struct DoctorBundle {
    pm: ProfileManager,
    profile_id: i64,
    raw_crash_log: Option<String>,
    context: DoctorContext,
}

impl DoctorBundle {
    async fn record_crash_if_present(&self) -> Result<()> {
        let Some(crash) = &self.context.crash else {
            return Ok(());
        };
        let Some(raw) = &self.raw_crash_log else {
            return Ok(());
        };
        self.pm
            .db()
            .record_crash_log(Some(self.profile_id), crash, raw)
            .await?;
        Ok(())
    }
}

async fn load_bundle(
    game: &str,
    profile_name: Option<String>,
    log_path: Option<PathBuf>,
    format: CrashFormatArg,
) -> Result<DoctorBundle> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let typed_game = GameId::from(game);
    let profile = load_profile(&pm, &typed_game, profile_name).await?;
    let profile_id = profile.id.ok_or_else(|| {
        anyhow::anyhow!("profile '{}' is not stored in the database", profile.name)
    })?;
    let active_plugin_entries = super::load_plugin_order(&pm, &profile).await?;
    let installed_files = pm.db().installed_files_for_profile(profile_id).await?;
    let tool_files = pm.db().load_all_applied_files(&typed_game).await?;
    let (diagnostics, collision_report) =
        analyze_profile(&pm, game, &profile, &active_plugin_entries).await?;
    let recent_profile_diffs = recent_diffs(&pm, profile_id, &profile).await?;

    let (crash, raw_crash_log) = if let Some(path) = log_path {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read crash log {}", path.display()))?;
        let report = modde_core::crash::correlate_crash_log(
            &path,
            &raw,
            format.into(),
            CrashCorrelationInput {
                game_id: game.to_string(),
                profile: profile.clone(),
                active_plugins: active_plugin_entries.clone(),
                installed_files: installed_files.clone(),
                tool_files: tool_files.clone(),
            },
        );
        (Some(report), Some(raw))
    } else {
        (None, None)
    };

    let context = build_doctor_context(DoctorContextInput {
        game_id: game.to_string(),
        profile,
        crash,
        active_plugins: active_plugin_entries,
        installed_files,
        tool_files,
        diagnostics,
        collision_report,
        recent_profile_diffs,
    });

    Ok(DoctorBundle {
        pm,
        profile_id,
        raw_crash_log,
        context,
    })
}

async fn load_profile(
    pm: &ProfileManager,
    game_id: &GameId,
    profile_name: Option<String>,
) -> Result<Profile> {
    if let Some(name) = profile_name {
        return Ok(pm.load(&name, Some(game_id)).await?);
    }
    let (_, name) = pm
        .db()
        .get_active_profile(game_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no active profile for game '{game_id}'"))?;
    Ok(pm.load(&name, Some(game_id)).await?)
}

async fn analyze_profile(
    pm: &ProfileManager,
    game_id: &str,
    profile: &Profile,
    active_plugin_entries: &[PluginEntry],
) -> Result<(Vec<Diagnostic>, Option<CollisionReport>)> {
    let store = paths::store_dir();
    let staging = ProfileManager::staging_dir(&profile.name);
    let hidden: HashSet<(String, String)> = match profile.id {
        Some(profile_id) => pm
            .db()
            .list_hidden_files(profile_id)
            .await?
            .into_iter()
            .map(|row| (row.mod_id, row.rel_path))
            .collect(),
        None => HashSet::new(),
    };
    let active_plugins = active_plugin_entries
        .iter()
        .filter(|plugin| plugin.enabled)
        .map(|plugin| plugin.plugin_name.clone())
        .collect::<Vec<_>>();
    let engine = match game_id {
        #[cfg(feature = "bethesda")]
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
            modde_games::bethesda::diagnostics::bethesda_diagnostics()
        }
        _ => modde_core::diagnostics::base_diagnostics(),
    };
    let classifier = modde_games::resolve_collision_classifier(game_id);
    let (diagnostics, analysis) = modde_core::diagnostics::run_profile_diagnostics(
        game_id,
        profile,
        &active_plugins,
        &store,
        &staging,
        &hidden,
        classifier.as_deref(),
        &engine,
    )?;
    Ok((diagnostics, analysis.collision_report))
}

async fn recent_diffs(
    pm: &ProfileManager,
    profile_id: i64,
    profile: &Profile,
) -> Result<Vec<modde_core::doctor::DoctorProfileDiff>> {
    let current = DoctorProfileModSnapshot::from_profile(profile);
    let snapshots = pm
        .db()
        .recent_profile_state_snapshots(profile_id, 5)
        .await?;
    for row in snapshots {
        let diffs = diff_profile_snapshots(&current, &row);
        if !diffs.is_empty() {
            return Ok(diffs);
        }
    }
    Ok(Vec::new())
}

fn print_profile_report(bundle: &DoctorBundle) {
    let ctx = &bundle.context;
    println!("Doctor profile: {} ({})", ctx.profile_name, ctx.game_id);
    println!(
        "Mods: {} enabled / {} total",
        ctx.mod_set.iter().filter(|m| m.enabled).count(),
        ctx.mod_set.len()
    );
    println!("Active plugins: {}", ctx.active_plugins.len());
    println!("Installed file records: {}", ctx.installed_files.len());
    println!("Tool-applied files: {}", ctx.tool_files.len());
    println!("Diagnostics: {}", ctx.diagnostics.len());
    println!("Collision pairs: {}", ctx.collisions.len());
    println!("Recent profile diffs: {}", ctx.recent_profile_diffs.len());
    println!("Evidence rows: {}", ctx.evidence.len());
    for diag in &ctx.diagnostics {
        println!("  [{}] {}", diag.severity, diag.title);
        if !diag.detail.is_empty() {
            println!("       {}", diag.detail);
        }
    }
}

fn print_crash_report(ctx: &DoctorContext) {
    let Some(report) = &ctx.crash else {
        println!("No crash log was analyzed.");
        return;
    };
    println!(
        "Crash log: {} ({}, sha256 {})",
        report.source_path.display(),
        report.format.as_str(),
        &report.raw_sha256[..12]
    );
    println!("Profile: {} ({})", report.profile_name, report.game_id);
    if let Some(line) = &report.signature.exception_line {
        println!("Exception: {line}");
    }
    if report.suspects.is_empty() {
        println!("No installed mod/plugin/DLL/asset correlation found.");
    } else {
        println!("{} correlated candidate(s):", report.suspects.len());
        for (idx, suspect) in report.suspects.iter().enumerate() {
            let name = suspect
                .display_name
                .as_deref()
                .or(suspect.mod_id.as_deref())
                .or(suspect.plugin_name.as_deref())
                .unwrap_or("unmatched crash evidence");
            println!("{}. [{:?}] {name}", idx + 1, suspect.confidence);
            if let Some(version) = &suspect.version {
                println!("   version: {version}");
            }
            if let Some(plugin) = &suspect.plugin_name {
                let order = suspect
                    .plugin_load_index
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                println!("   plugin: {plugin} (load order {order})");
            }
            for evidence in &suspect.evidence {
                println!(
                    "   - line {} [{}]: {} ({})",
                    evidence.line, evidence.section, evidence.token, evidence.reason
                );
            }
        }
    }
    println!("Evidence rows: {}", ctx.evidence.len());
}

fn print_explanation(explanation: &modde_core::doctor::DoctorExplanation) {
    if explanation.hypotheses.is_empty() {
        println!("No supported LLM hypotheses returned.");
    } else {
        println!("LLM-grounded hypotheses:");
        for h in &explanation.hypotheses {
            println!("{}. [{}] {}", h.rank, h.confidence, h.title);
            println!("   {}", h.summary);
            println!("   evidence: {}", h.evidence_ids.join(", "));
            if !h.recommended_checks.is_empty() {
                println!("   checks: {}", h.recommended_checks.join("; "));
            }
        }
    }
    if !explanation.unsupported.is_empty() {
        println!("Unsupported hypotheses rejected:");
        for item in &explanation.unsupported {
            println!("  - {} ({})", item.title, item.reason);
        }
    }
}
