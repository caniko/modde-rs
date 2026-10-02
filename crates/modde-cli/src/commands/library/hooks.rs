use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use modde_core::library::{LaunchSettings, LibraryPreferences, normalized_path};
use modde_games::library::{LibraryGame, Store};

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn install(game: &LibraryGame) -> Result<String> {
    ensure!(
        cfg!(unix),
        "store command wrappers currently require a Unix host"
    );
    ensure!(
        game.store != Store::Local,
        "local games use their configured executable"
    );
    game.install_path
        .as_ref()
        .context("game is not installed")?;
    let data = std::path::absolute(modde_core::paths::modde_data_dir())?;
    let config = std::path::absolute(modde_core::paths::config_dir())?;
    let directory = data.join("launch-hooks");
    std::fs::create_dir_all(&directory)?;
    let wrapper = directory.join(format!("{}.sh", game.id));
    let binary = std::env::current_exe()?;
    let script = format!(
        "#!/bin/sh\n# Generated exact-install command boundary; argv is never re-parsed.\nexec {} --config-dir {} --data-dir {} library wrap {} -- \"$@\"\n",
        quote(binary.to_str().context("non-UTF8 executable path")?),
        quote(config.to_str().context("non-UTF8 config path")?),
        quote(data.to_str().context("non-UTF8 data path")?),
        quote(&game.id)
    );
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    use std::io::Write;
    file.write_all(script.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }
    file.as_file().sync_all()?;
    file.persist(&wrapper)?;
    let note = if game.store == Store::Steam {
        format!(
            "Paste into this game's Steam Launch Options: {} %command%",
            quote(wrapper.to_str().context("non-UTF8 wrapper path")?)
        )
    } else if let Some(config_path) = modde_games::library::heroic_config(game)? {
        let game_id = &game.app_id;
        let bytes = std::fs::read(&config_path)?;
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let game_config = value
            .get_mut(game_id)
            .and_then(serde_json::Value::as_object_mut)
            .context("Heroic game config missing")?;
        let wrappers = game_config
            .entry("wrapperOptions")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("invalid Heroic wrapperOptions")?;
        let path = wrapper.to_str().context("non-UTF8 wrapper path")?;
        // fgmod is a preparation step; run after its DLL edits, before the
        // runner and game. Library restores the deployed proxy DLLs here.
        let legacy = data.join("bin/modde-launch-wrapper.sh");
        wrappers.retain(|entry| {
            entry
                .get("exe")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|exe| exe != path && Path::new(exe) != legacy)
        });
        let after_fgmod = wrappers
            .iter()
            .rposition(|entry| {
                entry
                    .get("exe")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|exe| exe.contains("fgmod"))
            })
            .map_or(0, |index| index + 1);
        wrappers.insert(after_fgmod, serde_json::json!({"exe": path, "args": ""}));
        let backup = config_path.with_extension("json.modde-before-hook");
        if !backup.exists() {
            std::fs::copy(&config_path, &backup)?;
        }
        modde_core::library::atomic_json(&config_path, &value)?;
        "Heroic wrapper installed. Disable automatic cloud-save sync for profile-managed saves, then restart Heroic to reload its saved configuration.".into()
    } else {
        format!(
            "Add a Heroic wrapper after fgmod (outermost if fgmod is absent): executable={}, arguments empty. Restart Heroic after saving.",
            wrapper.display()
        )
    };
    LibraryPreferences::update(|preferences| {
        preferences
            .launches
            .entry(game.id.clone())
            .or_default()
            .store_hook = true;
    })?;
    Ok(note)
}

/// Heroic downloads before entering our wrapper and uploads after it returns.
/// Both happen outside the selected profile's save transition (including bisect
/// source restoration), so the provider must relinquish automatic save writes.
pub(super) fn require_profile_save_boundary(game: &LibraryGame) -> Result<()> {
    if !matches!(game.store, Store::Gog | Store::Epic | Store::Sideload) {
        return Ok(());
    }
    let path = modde_games::library::heroic_config(game)?
        .context("profile-managed Heroic saves require a matching game configuration with automatic cloud-save sync disabled")?;
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    validate_cloud_sync(
        config
            .get(&game.app_id)
            .context("Heroic game config missing")?,
    )
}

fn validate_cloud_sync(settings: &serde_json::Value) -> Result<()> {
    ensure!(
        settings
            .get("autoSyncSaves")
            .and_then(serde_json::Value::as_bool)
            == Some(false),
        "disable automatic cloud-save sync for this game in Heroic and restart Heroic before using profile-managed saves; cloud sync runs outside the modde save boundary"
    );
    Ok(())
}

pub(super) fn prepare_boundary(
    game: &LibraryGame,
    settings: &mut LaunchSettings,
    prefix: Option<&Path>,
    command: &[OsString],
) -> Result<Option<PathBuf>> {
    ensure!(
        settings.store_hook,
        "generate this installation's store hook before using it"
    );
    ensure!(
        settings.executable.is_none() && settings.runner.is_none(),
        "store wrapper cannot also select a direct executable/runner"
    );
    command.first().context("store command is empty")?;
    ensure!(
        !command
            .iter()
            .any(|arg| arg.as_encoded_bytes().contains(&0)),
        "NUL in store command"
    );
    ensure!(
        !command
            .iter()
            .any(|arg| Path::new(arg).file_name().is_some_and(|name| name
                == "modde-launch-wrapper.sh"
                || name == "modde-launch-wrapper.cmd")),
        "remove the legacy modde launch wrapper from the store command; the Library hook owns save capture"
    );
    if game.store == Store::Steam {
        if let Ok(id) = std::env::var("SteamAppId") {
            ensure!(
                id == game.app_id,
                "Steam wrapper belongs to a different game"
            );
        }
    } else if matches!(game.store, Store::Gog | Store::Epic | Store::Sideload) {
        if let Ok(id) = std::env::var("HEROIC_APP_NAME") {
            ensure!(
                id == game.app_id,
                "Heroic wrapper belongs to a different game"
            );
        }
        if let Ok(runner) = std::env::var("HEROIC_APP_RUNNER") {
            let expected = match game.store {
                Store::Gog => "gog",
                Store::Epic => "legendary",
                _ => "sideload",
            };
            ensure!(
                runner == expected,
                "Heroic wrapper belongs to a different store"
            );
        }
    }
    let wine_environment = std::env::var_os("WINEPREFIX");
    let inherited_prefix = boundary_prefix(
        wine_environment.clone(),
        std::env::var_os("STEAM_COMPAT_DATA_PATH"),
    )?;
    if let Some(actual) = inherited_prefix {
        let expected = prefix.map(Path::to_path_buf).or_else(|| {
            if game.store == Store::Steam {
                modde_games::library::context::steam_prefix(
                    game.install_path.as_deref()?,
                    &game.app_id,
                )
            } else {
                None
            }
        });
        ensure!(
            expected.as_deref().map(normalized_path) == Some(normalized_path(&actual)),
            "store Wine prefix differs from saved installation; configure prefix {} before launching",
            actual.display()
        );
        settings.prefix = Some(actual);
    }
    // With no Wine environment, retain only an explicitly configured prefix.
    // An old Steam compatdata directory alone does not make this a Wine launch.
    if settings.working_directory.is_none() {
        settings.working_directory = Some(std::env::current_dir()?);
    }
    for (key, value) in std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
    {
        if inherited_environment_key(&key) {
            settings.environment.entry(key).or_insert(value);
        }
    }
    // Preserve Proton/UMU's runner-facing container root while settings.prefix
    // pins its physical pfx for saves. launch validation checks both together.
    // Source: https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/3934a83a0707baad23cd2c06bc94bd23f51e6622/src/backend/launcher.ts#L1110-L1141
    inherit_wine_prefix(settings, wine_environment)?;
    if settings.sandbox.enabled {
        // Explicit runtime roots supplied by the store, not the user's home.
        for key in ["STEAM_COMPAT_CLIENT_INSTALL_PATH", "PROTONPATH"] {
            if let Some(value) = settings.environment.get(key) {
                let path = PathBuf::from(value);
                if path.is_absolute() && path.is_dir() {
                    settings.sandbox.read_only.push(path);
                }
            }
        }
        for key in ["STEAM_COMPAT_TOOL_PATHS", "STEAM_COMPAT_MOUNTS"] {
            if let Some(value) = settings.environment.get(key) {
                settings
                    .sandbox
                    .read_only
                    .extend(std::env::split_paths(value).filter(|p| p.is_absolute() && p.exists()));
            }
        }
        if let Some(value) = settings.environment.get("STEAM_COMPAT_DATA_PATH") {
            let path = PathBuf::from(value);
            if path.is_absolute() && path.is_dir() {
                settings.sandbox.writable.push(path);
            }
        }
    }
    Ok(settings.prefix.clone())
}

fn inherit_wine_prefix(settings: &mut LaunchSettings, wine: Option<OsString>) -> Result<()> {
    let Some(wine) = wine else {
        return Ok(());
    };
    let wine = wine
        .into_string()
        .map_err(|_| anyhow::anyhow!("store WINEPREFIX is not UTF-8"))?;
    if let Some(saved) = settings.environment.get("WINEPREFIX") {
        ensure!(
            normalized_path(Path::new(saved)) == normalized_path(Path::new(&wine)),
            "saved WINEPREFIX differs from the store's runner-facing prefix; remove that environment override and use the prefix field for save discovery"
        );
    }
    settings.environment.insert("WINEPREFIX".into(), wine);
    Ok(())
}

fn inherited_environment_key(key: &str) -> bool {
    // WINEPREFIX is forwarded separately after resolving the physical prefix.
    // Preserve runtime tunings even when bubblewrap clears ambient variables.
    key != "WINEPREFIX"
        && ([
            "STEAM_", "Steam", "PROTON_", "WINE", "DXVK_", "VKD3D_", "UMU_", "SDL_", "MANGOHUD",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
            || matches!(
                key,
                "GAMEID"
                    | "STORE"
                    | "PROTONPATH"
                    | "LD_PRELOAD"
                    | "ORIG_LD_LIBRARY_PATH"
                    | "GST_PLUGIN_SYSTEM_PATH_1_0"
                    | "HEROIC_APP_NAME"
                    | "HEROIC_APP_RUNNER"
                    | "HEROIC_APP_SOURCE"
                    | "HOME"
                    | "XDG_CONFIG_HOME"
                    | "XDG_DATA_HOME"
                    | "XDG_CACHE_HOME"
            ))
}

fn boundary_prefix(wine: Option<OsString>, compat: Option<OsString>) -> Result<Option<PathBuf>> {
    let wine = wine.map(PathBuf::from);
    let compat = compat.map(PathBuf::from);
    for path in wine.iter().chain(compat.iter()) {
        ensure!(path.is_absolute(), "store prefix must be an absolute path");
    }
    if let Some(compat) = compat {
        let prefix = compat.join("pfx");
        // Heroic's Proton boundary exports both variables as the container;
        // Steam may export WINEPREFIX as its pfx child. UMU uses a pfx alias.
        ensure!(
            wine.as_deref()
                .is_none_or(|wine| normalized_path(wine) == normalized_path(&compat)
                    || normalized_path(wine) == normalized_path(&prefix)),
            "store Wine and Proton prefixes disagree"
        );
        Ok(Some(normalized_path(&prefix)))
    } else {
        Ok(wine)
    }
}

/// Restore only resolved VFS winners, rather than scanning raw (possibly
/// disabled or losing) mods. Normal deployment already restores these files.
pub(super) fn restore_deployed_dlls(
    profile: &modde_core::profile::Profile,
    install: &Path,
) -> Result<()> {
    use modde_core::profile::{ProfileManager, ProfileSource};
    ensure!(
        !matches!(profile.source, ProfileSource::Wabbajack { .. }),
        "Wabbajack store launches require deployment to restore the resolved files; omit --no-deploy"
    );
    let plugin = modde_games::resolve_game_plugin(profile.game_id.as_str())
        .context("game plugin unavailable")?;
    let destination = plugin.executable_dir(install);
    let relative = destination
        .strip_prefix(install)
        .context("executable directory is outside this installation")?;
    let staging = ProfileManager::staging_dir(&profile.name);
    ensure!(
        staging.is_dir(),
        "there is no resolved deployment to restore; omit --no-deploy"
    );
    let source_dir = modde_core::fs::case_match_path(&staging, relative)?;
    if !source_dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !modde_games::tools::optiscaler::FGMOD_DELETED_DLLS
            .iter()
            .any(|dll| name.eq_ignore_ascii_case(dll))
        {
            continue;
        }
        let source = entry.path().canonicalize()?;
        let target = modde_core::fs::case_match_path(&destination, Path::new(&name))?;
        if target.canonicalize().ok().as_ref() == Some(&source) {
            continue;
        }
        let temporary = tempfile::NamedTempFile::new_in(&destination)?;
        std::fs::copy(&source, temporary.path())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&target)
            .with_context(|| format!("restoring deployed DLL {}", target.display()))?;
    }
    Ok(())
}

/// A pre-existing Wine server can execute the new client outside our process
/// tree. Require an idle prefix before treating descendant exit as completion.
pub(super) fn require_idle_prefix(prefix: Option<&Path>) -> Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(prefix) = prefix {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::MetadataExt;
        let prefix = normalized_path(prefix);
        let uid = std::fs::metadata("/proc/self")?.uid();
        for entry in std::fs::read_dir("/proc")?.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            if !entry.metadata().is_ok_and(|metadata| metadata.uid() == uid) {
                continue;
            }
            let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            if !name.starts_with("wine") {
                continue;
            }
            let environment = match std::fs::read(entry.path().join("environ")) {
                Ok(environment) => environment,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("cannot establish the prefix of Wine process {pid}")
                    });
                }
            };
            // A zombie has already exited and cannot accept another Wine client.
            if environment.is_empty()
                && std::fs::read_to_string(entry.path().join("status")).is_ok_and(|status| {
                    status
                        .lines()
                        .any(|line| line.starts_with("State:") && line.contains("(zombie)"))
                })
            {
                continue;
            }
            let value = |key: &[u8]| {
                environment
                    .split(|byte| *byte == 0)
                    .find_map(|value| value.strip_prefix(key))
                    .map(|value| PathBuf::from(std::ffi::OsStr::from_bytes(value)))
            };
            let actual = value(b"WINEPREFIX=")
                .or_else(|| value(b"HOME=").map(|home| home.join(".wine")))
                .with_context(|| format!("cannot establish the prefix of Wine process {pid}"))?;
            let actual = if actual.is_absolute() {
                actual
            } else {
                std::fs::read_link(entry.path().join("cwd"))?.join(actual)
            };
            if normalized_path(&actual) == prefix {
                bail!(
                    "Wine prefix is already in use by PID {pid}; close its game and Wine services before launching"
                );
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = prefix;
    Ok(())
}

/// Wine tools can return just before their helper services shut down. Give
/// those descendants a bounded grace period after patching, without killing a
/// server or admitting an in-use prefix into a new observer's process tree.
pub(super) fn wait_for_idle_prefix(prefix: Option<&Path>) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        match require_idle_prefix(prefix) {
            Ok(()) => return Ok(()),
            Err(error) if start.elapsed() >= std::time::Duration::from_secs(10) => {
                return Err(error);
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_paths_do_not_interpolate_shell_fragments() {
        assert_eq!(quote("/games/a'b $(bad)"), "'/games/a'\\''b $(bad)'");
    }

    #[test]
    fn heroic_proton_container_and_steam_wine_environment_resolve_the_same_prefix() {
        let root = tempfile::tempdir().unwrap();
        let compat = root.path().join("compat");
        let prefix = compat.join("pfx");
        for wine in [
            None,
            Some(compat.as_os_str().to_owned()),
            Some(prefix.as_os_str().to_owned()),
        ] {
            assert_eq!(
                boundary_prefix(wine, Some(compat.as_os_str().to_owned())).unwrap(),
                Some(prefix.clone())
            );
        }
        assert!(
            boundary_prefix(
                Some(root.path().join("another").into_os_string()),
                Some(compat.into_os_string())
            )
            .is_err()
        );
        assert_eq!(boundary_prefix(None, None).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn umu_prefix_alias_resolves_to_the_initialized_wine_root() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        std::os::unix::fs::symlink(&prefix, prefix.join("pfx")).unwrap();
        for wine in [&prefix, &prefix.join("pfx")] {
            assert_eq!(
                boundary_prefix(
                    Some(wine.as_os_str().to_owned()),
                    Some(prefix.as_os_str().to_owned())
                )
                .unwrap(),
                Some(prefix.clone())
            );
        }
    }

    #[test]
    fn a_saved_physical_prefix_cannot_replace_the_stores_runner_container() {
        let root = tempfile::tempdir().unwrap();
        let compat = root.path().join("compat");
        let prefix = compat.join("pfx");
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        let mut settings = LaunchSettings {
            prefix: Some(prefix.clone()),
            ..Default::default()
        };
        settings
            .environment
            .insert("WINEPREFIX".into(), prefix.to_string_lossy().into_owned());
        assert!(inherit_wine_prefix(&mut settings, Some(compat.as_os_str().to_owned())).is_err());
        settings.environment.remove("WINEPREFIX");
        inherit_wine_prefix(&mut settings, Some(compat.as_os_str().to_owned())).unwrap();
        assert_eq!(
            settings.environment.get("WINEPREFIX").map(String::as_str),
            compat.to_str()
        );
        assert_eq!(settings.prefix, Some(prefix));
    }

    #[test]
    fn boundary_preserves_runner_tunings_without_forwarding_store_credentials() {
        for key in [
            "WINEESYNC",
            "WINEFSYNC",
            "DXVK_ASYNC",
            "VKD3D_CONFIG",
            "UMU_RUNTIME_UPDATE",
            "PROTONPATH",
            "STEAM_COMPAT_DATA_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "HEROIC_APP_NAME",
        ] {
            assert!(inherited_environment_key(key), "{key}");
        }
        for key in ["WINEPREFIX", "MODDE_STEAM_API_KEY", "AWS_SECRET_ACCESS_KEY"] {
            assert!(!inherited_environment_key(key), "{key}");
        }
    }

    #[test]
    fn profile_save_boundary_requires_explicitly_disabled_heroic_cloud_sync() {
        for settings in [
            serde_json::json!({}),
            serde_json::json!({"autoSyncSaves": true}),
            serde_json::json!({"autoSyncSaves": "false"}),
        ] {
            assert!(validate_cloud_sync(&settings).is_err());
        }
        assert!(validate_cloud_sync(&serde_json::json!({"autoSyncSaves": false})).is_ok());
    }
}
