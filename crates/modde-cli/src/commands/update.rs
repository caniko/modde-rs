use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{info, warn};

use std::io::{self, BufRead, Write};

use modde_core::ModdeDb;
use modde_core::installer::InstallStatus;
use modde_core::profile::{LockReason, ProfileManager};
use modde_sources::nexus::api::NexusApi;
use modde_sources::nexus::auth::load_api_key;
use modde_sources::nexus::install::{InstallOutcome, install_single_mod};
use modde_sources::nexus::updates::{TrackedMod, check_updates};

use super::load_profile_or_default;

pub async fn handle_product_check() -> Result<()> {
    let settings = modde_core::settings::AppSettings::load();
    if !modde_core::update_check::update_checks_enabled(&settings) {
        println!("modde update checks are disabled.");
        return Ok(());
    }

    match modde_core::update_check::check_latest_uncached(&settings).await? {
        Some(update) => {
            println!(
                "modde {} is available (current: {}).",
                update.latest_version, update.current_version
            );
            println!("{}", update.release_url);
        }
        None => {
            println!("modde {} is up to date.", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}

pub async fn handle_check(
    profile_name: Option<String>,
    game_id: Option<String>,
    period: String,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;

    info!(profile = %profile.name, game = %profile.game_id, "checking for mod updates");

    // Collect tracked mods with Nexus metadata
    let tracked: Vec<TrackedMod> = profile
        .mods
        .iter()
        .filter(|m| m.enabled && m.nexus_mod_id.is_some())
        .filter_map(|m| {
            Some(TrackedMod {
                mod_id: m.mod_id.clone(),
                nexus_mod_id: m.nexus_mod_id?,
                nexus_game_domain: m.nexus_game_domain.clone()?,
                installed_version: m.version.clone(),
                installed_timestamp: m.installed_timestamp?,
            })
        })
        .collect();

    if tracked.is_empty() {
        println!(
            "No mods with Nexus metadata found in profile '{}'.",
            profile.name
        );
        println!("Hint: Mods installed via 'modde install mod' or Nexus Collections");
        println!("      automatically track their Nexus source for update checking.");
        return Ok(());
    }

    println!(
        "Checking updates for {} tracked mods (period: {period})...",
        tracked.len()
    );

    let api_key = load_api_key()?;
    let client = Client::new();
    let api = NexusApi::new(client, api_key);

    let updates = check_updates(&api, &tracked, &period).await?;

    if updates.is_empty() {
        println!("All {} tracked mods are up to date.", tracked.len());
    } else {
        println!("\n{} mod(s) have updates available:\n", updates.len());
        for u in &updates {
            let installed = u.installed_version.as_deref().unwrap_or("unknown");
            println!(
                "  {} (installed: {}, updated: {})",
                u.mod_id, installed, u.latest_file_update
            );
        }
    }

    Ok(())
}

pub struct ApplyOptions {
    pub profile_name: Option<String>,
    pub game_id: Option<String>,
    pub period: String,
    pub dry_run: bool,
    pub safety: ApplySafety,
}

pub struct ApplySafety {
    pub confirm_locked: bool,
    pub accept_breaking: bool,
    pub yes: bool,
}

/// Download and install the newest MAIN file for every tracked mod that
/// has a newer file on Nexus, then rewrite the profile entries to point
/// at the fresh `(mod_id, file_id)` pair.
///
/// This is the auto-update half of `modde update check`: pair them in a
/// cron / scheduled task and the manager will keep itself current.
pub async fn handle_apply(options: ApplyOptions) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let mut profile = load_profile_or_default(
        &pm,
        options.profile_name.as_deref(),
        options.game_id.as_deref(),
    )
    .await?;

    info!(
        profile = %profile.name,
        game = %profile.game_id,
        dry_run = options.dry_run,
        confirm_locked = options.safety.confirm_locked,
        accept_breaking = options.safety.accept_breaking,
        yes = options.safety.yes,
        "applying mod updates"
    );

    // Lock guard. Wabbajack / Collection / TOML-import profiles ship a
    // pinned, authoritative load order; auto-updating mods inside one
    // would silently drift the profile away from its source. Refuse
    // unless the user explicitly opts in.
    if let Some(lock) = profile.load_order_lock.as_ref() {
        if !options.safety.confirm_locked {
            let label = match &lock.reason {
                LockReason::Wabbajack { .. } => "Wabbajack",
                LockReason::NexusCollection { .. } => "Nexus Collection",
                LockReason::TomlImport { .. } => "TOML import",
                LockReason::Manual { .. } => "manual",
            };
            anyhow::bail!(
                "profile '{}' is locked ({label}); auto-updating its mods would drift it from \
                 the authoritative source. Re-run with --confirm-locked to override, or \
                 `modde profile unlock {}` first.",
                profile.name,
                profile.name
            );
        }
        eprintln!(
            "  warning: profile '{}' is locked — proceeding with --confirm-locked. \
             The locked source will not be re-verified after this update.",
            profile.name
        );
    }

    let tracked: Vec<TrackedMod> = profile
        .mods
        .iter()
        .filter(|m| m.enabled && m.nexus_mod_id.is_some())
        .filter_map(|m| {
            Some(TrackedMod {
                mod_id: m.mod_id.clone(),
                nexus_mod_id: m.nexus_mod_id?,
                nexus_game_domain: m.nexus_game_domain.clone()?,
                installed_version: m.version.clone(),
                installed_timestamp: m.installed_timestamp?,
            })
        })
        .collect();

    if tracked.is_empty() {
        println!(
            "No mods with Nexus metadata found in profile '{}'.",
            profile.name
        );
        return Ok(());
    }

    let api_key = load_api_key()?;
    let client = Client::new();
    let api = NexusApi::new(client.clone(), api_key.clone());

    let updates = check_updates(&api, &tracked, &options.period).await?;

    if updates.is_empty() {
        println!("All {} tracked mods are up to date.", tracked.len());
        return Ok(());
    }

    println!("{} mod(s) have updates available.\n", updates.len());

    if options.dry_run {
        for u in &updates {
            let installed = u.installed_version.as_deref().unwrap_or("unknown");
            println!(
                "  would update {} (installed: {}, latest_file_update: {})",
                u.mod_id, installed, u.latest_file_update
            );
        }
        println!("\nDry run — re-run without --dry-run to apply.");
        return Ok(());
    }

    // Build a domain -> nexus_mod_id list so we can fetch the latest
    // MAIN file in one API round-trip per mod.
    let mut applied = 0usize;
    let mut failed = 0usize;
    let db = ModdeDb::open().await.context("failed to open mod db")?;
    let profile_id = profile
        .id
        .ok_or_else(|| anyhow::anyhow!("profile has no database id"))?;

    for u in &updates {
        // Recover the domain from the profile entry: ModUpdate doesn't
        // carry it, but the local `mod_id` matches an EnabledMod row.
        let Some(game_domain) = profile
            .mods
            .iter()
            .find(|m| m.mod_id == u.mod_id)
            .and_then(|m| m.nexus_game_domain.clone())
        else {
            warn!(mod_id = %u.mod_id, "skipping update: missing nexus_game_domain");
            failed += 1;
            continue;
        };

        // Pick the newest MAIN file. Mirrors `commands::install::handle_single_mod`.
        let files = match api.get_mod_files(&game_domain, u.nexus_mod_id).await {
            Ok(f) => f,
            Err(e) => {
                warn!(mod_id = %u.mod_id, error = %e, "failed to fetch mod files");
                failed += 1;
                continue;
            }
        };
        let mut candidates: Vec<_> = files
            .files
            .into_iter()
            .filter(|f| f.category_name.as_deref() == Some("MAIN"))
            .collect();
        candidates.sort_by_key(|f| std::cmp::Reverse(f.uploaded_timestamp));
        let Some(file) = candidates.into_iter().next() else {
            warn!(mod_id = %u.mod_id, "no MAIN file available, skipping");
            failed += 1;
            continue;
        };
        let new_file_id = file.file_id;
        let new_mod_id_str = format!("{game_domain}_{}_{new_file_id}", u.nexus_mod_id);

        // If we already have this exact (mod_id, file_id) row, nothing
        // to do. This can happen if `update check` reported a generic
        // mod_activity bump without an actual new MAIN file.
        if profile.mods.iter().any(|m| m.mod_id == new_mod_id_str) {
            info!(mod_id = %new_mod_id_str, "already at latest file id, skipping");
            continue;
        }

        let mod_info = match api.get_mod(&game_domain, u.nexus_mod_id).await {
            Ok(m) => m,
            Err(e) => {
                warn!(mod_id = %u.mod_id, error = %e, "failed to fetch mod info");
                failed += 1;
                continue;
            }
        };

        // Breaking-version gate. A semver major bump is treated as a
        // potentially save- or load-order-incompatible change. We
        // require both the `--accept-breaking` flag (broad opt-in) and
        // a per-mod y/N confirmation (precise opt-in). `--yes` skips
        // the per-mod prompt only when `--accept-breaking` is also set.
        if let Some(installed) = u.installed_version.as_deref()
            && is_breaking_bump(installed, &mod_info.version)
        {
            if !options.safety.accept_breaking {
                println!(
                    "  skipping {}: breaking version bump {} → {}. Pass --accept-breaking \
                     to allow it.",
                    u.mod_id, installed, mod_info.version
                );
                failed += 1;
                continue;
            }
            let approved = if options.safety.yes {
                true
            } else {
                confirm_breaking(&u.mod_id, installed, &mod_info.version)?
            };
            if !approved {
                println!("  skipped {} (user declined breaking update).", u.mod_id);
                failed += 1;
                continue;
            }
        }

        let probe = modde_games::resolve_game_plugin(&game_domain)
            .or_else(|| modde_games::resolve_game_plugin_by_nexus_domain(&game_domain))
            .map_or_else(
                modde_core::installer::InstallProbe::noop,
                modde_games::game_probe,
            );

        println!(
            "Updating {} → file {} ({})",
            u.mod_id, new_file_id, mod_info.version
        );

        let outcome = match install_single_mod(
            &client,
            &api_key,
            &game_domain,
            u.nexus_mod_id,
            new_file_id,
            &mod_info,
            &probe,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                warn!(mod_id = %u.mod_id, error = %e, "install failed");
                failed += 1;
                continue;
            }
        };

        match &outcome {
            InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => {}
            InstallOutcome::PendingUserInput { method } => {
                println!(
                    "  {} requires user input ({method}); open the UI to finish.",
                    u.mod_id
                );
            }
            InstallOutcome::Unknown { dossier_path, .. } => {
                println!(
                    "  {} has an unknown layout. Dossier: {}",
                    u.mod_id,
                    dossier_path.display()
                );
            }
        }

        // Rewrite the profile entry: same enabled/notes/etc. but new
        // `mod_id`, `file_id`, `version`, and `installed_timestamp`.
        if let Some(slot) = profile.mods.iter_mut().find(|m| m.mod_id == u.mod_id) {
            slot.mod_id = new_mod_id_str.clone();
            slot.nexus_file_id = Some(new_file_id);
            slot.version = Some(mod_info.version.clone());
            slot.installed_timestamp = file.uploaded_timestamp.map(|t| t as i64);
            slot.install_status = Some(outcome.status());
        }

        if let InstallOutcome::Installed(plan) = &outcome {
            db.record_install(
                profile_id,
                &modde_core::ModId::from(new_mod_id_str.as_str()),
                plan,
                InstallStatus::Installed,
            )
            .await
            .context("failed to persist install plan")?;
        }

        applied += 1;
    }

    pm.create_or_update(&profile).await?;

    println!(
        "\nApplied {applied} update(s); {failed} failure(s); {} unchanged.",
        updates.len() - applied - failed
    );

    Ok(())
}

/// Best-effort detection of a breaking semver bump. Returns true when
/// the leading numeric component (major) increases. Prefix `v` and any
/// non-numeric tail (e.g. `1.0.0-beta`) are tolerated. Non-semver
/// strings fall through as "not breaking" — callers should treat that
/// as a soft signal, not a guarantee.
fn is_breaking_bump(installed: &str, latest: &str) -> bool {
    let major = |s: &str| -> Option<u64> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let head = s.split(['.', '-', '+', ' ']).next()?;
        head.parse::<u64>().ok()
    };
    match (major(installed), major(latest)) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    }
}

/// Per-mod y/N prompt for a breaking update. Reads one line from stdin;
/// only `y`/`yes` (case-insensitive) approves. Anything else (including
/// EOF) declines.
fn confirm_breaking(mod_id: &str, installed: &str, latest: &str) -> Result<bool> {
    print!("  [breaking] {mod_id} {installed} → {latest} — apply? [y/N] ");
    io::stdout().flush().ok();
    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let answer = line.trim().to_ascii_lowercase();
    Ok(matches!(answer.as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::is_breaking_bump;

    #[test]
    fn major_bump_is_breaking() {
        assert!(is_breaking_bump("1.2.3", "2.0.0"));
        assert!(is_breaking_bump("v1.0", "v2.0"));
        assert!(is_breaking_bump("0.9", "1.0"));
    }

    #[test]
    fn minor_or_patch_bump_is_not_breaking() {
        assert!(!is_breaking_bump("1.2.3", "1.3.0"));
        assert!(!is_breaking_bump("1.2.3", "1.2.4"));
        assert!(!is_breaking_bump("2.0", "2.0"));
    }

    #[test]
    fn non_semver_falls_through_as_safe() {
        assert!(!is_breaking_bump("RC1", "RC2"));
        assert!(!is_breaking_bump("alpha", "beta"));
    }
}
