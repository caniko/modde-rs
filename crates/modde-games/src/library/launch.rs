use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result, bail};
use modde_core::library::LaunchSettings;

use super::{LibraryGame, Store};

#[derive(Debug)]
pub enum LaunchOutcome {
    /// Store hand-off is not evidence that a game process started or exited.
    Requested,
    Exited(ExitStatus),
}

/// Constructed and preflighted before any live save/profile changes.
pub enum PreparedLaunch {
    Direct(Command),
    Store(String),
}

pub fn prepare(game: &LibraryGame, settings: &LaunchSettings, games: &[LibraryGame]) -> Result<PreparedLaunch> {
    if settings.executable.is_some() {
        Ok(PreparedLaunch::Direct(prepare_boundary(game, settings, games, &direct_arguments(settings)?)?))
    } else {
        preflight(game, settings, games)?;
        Ok(PreparedLaunch::Store(store_uri(game, false)?))
    }
}

/// Fail before profile/save mutation. In particular, never wrap a store URI in
/// bubblewrap and pretend that the process created by the store is contained.
pub fn validate(game: &LibraryGame, settings: &LaunchSettings, games: &[LibraryGame]) -> Result<()> {
    let install = game.install_path.as_deref().context("game is not installed")?;
    if !install.is_absolute() || !install.is_dir() { bail!("installation is missing: {}", install.display()); }
    if settings.save_directory.as_ref().is_some_and(|path| !path.is_absolute()) { bail!("save directory must be absolute"); }
    if settings.profile.as_ref().is_some_and(|profile| profile.trim().is_empty()) { bail!("profile name must not be empty"); }
    if settings.executable.is_some() || settings.store_hook {
        super::runtime::RuntimePaths::resolve(settings)?;
        if let Some(executable) = &settings.executable {
            require_file(executable, "executable")?;
            require_runnable(settings.runner.as_deref().unwrap_or(executable))?;
        }
        if settings.runner.is_some() && settings.prefix.is_none() { bail!("configure an explicit Wine/umu prefix before launching"); }
        if let Some(prefix) = &settings.prefix {
            if !prefix.is_absolute() || (prefix.exists() && !prefix.is_dir()) || modde_core::library::normalized_path(prefix).parent().is_none() {
                bail!("Wine prefix must be an absolute directory below the filesystem root");
            }
        }
        let working = settings.working_directory.as_deref().unwrap_or(install);
        if !working.is_absolute() || !working.is_dir() { bail!("working directory must exist and be absolute"); }
        for (key, value) in &settings.environment {
            if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
                bail!("invalid environment variable name or value");
            }
        }
        if settings.environment.get("UMU_CONTAINER_NSENTER").is_some_and(|value| value == "1") {
            bail!("launching through an existing UMU service cannot prove descendant exit; disable UMU_CONTAINER_NSENTER");
        }
        wine_environment_prefix(settings)?;
        if settings.arguments.iter().any(|value| value.contains('\0')) { bail!("argument contains a NUL byte"); }
        if settings.wrappers.iter().any(|wrapper| wrapper.first().is_none_or(String::is_empty) || wrapper.iter().any(|value| value.contains('\0'))) {
            bail!("wrapper must contain a program and valid arguments");
        }
        for wrapper in &settings.wrappers { resolve_wrapper(&wrapper[0], settings)?; }
        if settings.sandbox.enabled { super::sandbox::validate(settings)?; }
    } else {
        if settings.sandbox.enabled {
            bail!("sandboxing requires a direct game executable and runner; store URI launches cannot be sandboxed");
        }
        if settings.runner.is_some() || settings.prefix.is_some() || !settings.arguments.is_empty() || !settings.wrappers.is_empty()
            || !settings.environment.is_empty() || settings.working_directory.is_some() {
            bail!("set a direct executable to apply modde launch overrides, or edit launch options in the store");
        }
        if game.store == Store::Local { bail!("choose a game executable in Launch Settings"); }
        store_uri(game, false)?;
        if games.iter().filter(|other| other.entitlement == game.entitlement && other.install_path.is_some()).count() > 1 {
            bail!("this store has multiple installations; choose a direct executable to target this installation exactly");
        }
    }
    if settings.executable.is_none() && settings.store_hook {
        if game.store == Store::Local { bail!("a local installation needs an executable"); }
        if games.iter().filter(|other| other.entitlement == game.entitlement && other.install_path.is_some()).count() > 1 {
            bail!("multiple installations of this store entitlement require direct launch");
        }
    }
    Ok(())
}

fn resolve_wrapper(program: &str, settings: &LaunchSettings) -> Result<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() { require_runnable(path)?; return Ok(path.into()); }
    if path.components().count() != 1 { bail!("wrapper program must be absolute or a PATH command"); }
    let search = settings.environment.get("PATH").map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    for directory in std::env::split_paths(&search).filter(|dir| dir.is_absolute()) {
        let candidate = directory.join(path);
        if require_runnable(&candidate).is_ok() { return Ok(candidate); }
    }
    bail!("wrapper program is unavailable: {program}")
}

pub fn validate_boundary(command: &[std::ffi::OsString], settings: &LaunchSettings) -> Result<()> {
    let program = command.first().context("store command is empty")?;
    resolve_program(program, settings)?;
    if command.iter().any(|arg| arg.as_encoded_bytes().contains(&0)) { bail!("NUL byte in game command"); }
    Ok(())
}

pub fn resolve_program(program: &std::ffi::OsStr, settings: &LaunchSettings) -> Result<PathBuf> {
    if Path::new(program).is_absolute() {
        require_runnable(Path::new(program))?;
        Ok(program.into())
    } else {
        resolve_wrapper(program.to_str().context("non-UTF8 PATH program")?, settings)
    }
}

fn require_file(path: &Path, label: &str) -> Result<()> {
    if !path.is_absolute() || !path.is_file() { bail!("{label} must be an existing absolute file: {}", path.display()); }
    Ok(())
}

fn require_runnable(path: &Path) -> Result<()> {
    require_file(path, "program")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.metadata()?.permissions().mode() & 0o111 == 0 { bail!("program is not executable: {}", path.display()); }
    }
    Ok(())
}

/// Save/deployment paths use the physical Wine prefix. Proton and UMU may need
/// the containing compat directory in WINEPREFIX instead. Accept that spelling
/// only when the explicit compat environment proves the same save destination.
/// Source: https://github.com/Open-Wine-Components/umu-launcher/blob/e2b203a1fdd2af9f35166f5713cb3f85d72587e2/umu/umu_run.py#L79-L115
pub(super) fn wine_environment_prefix(settings: &LaunchSettings) -> Result<Option<&Path>> {
    use modde_core::library::normalized_path;
    let prefix = settings.prefix.as_deref();
    let compat = settings.environment.get("STEAM_COMPAT_DATA_PATH").map(Path::new);
    if let Some(compat) = compat {
        if !compat.is_absolute() || normalized_path(compat).parent().is_none()
            || prefix.map(normalized_path) != Some(normalized_path(&compat.join("pfx"))) {
            bail!("STEAM_COMPAT_DATA_PATH must contain the configured prefix as its pfx directory");
        }
    }
    if let Some(wine) = settings.environment.get("WINEPREFIX").map(Path::new) {
        if !wine.is_absolute() || (prefix.map(normalized_path) != Some(normalized_path(wine))
            && compat.is_none_or(|compat| normalized_path(compat) != normalized_path(wine))) {
            bail!("set the prefix field instead of redirecting WINEPREFIX to another save destination");
        }
        return Ok(Some(wine));
    }
    Ok(prefix)
}

pub fn direct_command(game: &LibraryGame, settings: &LaunchSettings) -> Result<Command> {
    boundary_command(game, settings, &direct_arguments(settings)?)
}

fn direct_arguments(settings: &LaunchSettings) -> Result<Vec<std::ffi::OsString>> {
    let executable = settings.executable.as_deref().context("direct executable is not configured")?;
    let mut argv = vec![settings.runner.as_deref().unwrap_or(executable).as_os_str().to_owned()];
    if settings.runner.is_some() { argv.push(executable.as_os_str().to_owned()); }
    Ok(argv)
}

/// Probe the complete sandbox, including discovered wrapper/runtime grants,
/// before deployment or save replacement. Command-only callers do not execute.
pub fn prepare_boundary(game: &LibraryGame, settings: &LaunchSettings, games: &[LibraryGame], supplied: &[std::ffi::OsString]) -> Result<Command> {
    validate(game, settings, games)?;
    validate_boundary(supplied, settings)?;
    boundary_command_inner(game, settings, supplied, true)
}

/// Store wrappers forward the exact argv from Steam/Heroic; never parse it as
/// shell syntax or insert a second Wine runner around the store command.
pub fn boundary_command(game: &LibraryGame, settings: &LaunchSettings, supplied: &[std::ffi::OsString]) -> Result<Command> {
    boundary_command_inner(game, settings, supplied, false)
}

fn boundary_command_inner(game: &LibraryGame, settings: &LaunchSettings, supplied: &[std::ffi::OsString], probe: bool) -> Result<Command> {
    let (program, arguments) = supplied.split_first().context("store supplied no game command")?;
    let install = game.install_path.as_deref().context("game is not installed")?;
    let mut argv: Vec<std::ffi::OsString> = Vec::new();
    let mut sandbox_settings = settings.clone();
    for wrapper in &settings.wrappers {
        let program = wrapper.first().context("empty wrapper")?;
        argv.push(sandbox_program(resolve_wrapper(program, settings)?, &mut sandbox_settings)?);
        argv.extend(wrapper[1..].iter().map(Into::into));
    }
    let program = if Path::new(program).is_absolute() { PathBuf::from(program) }
        else { resolve_wrapper(program.to_str().context("non-UTF8 PATH program")?, settings)? };
    argv.push(sandbox_program(program, &mut sandbox_settings)?);
    for argument in arguments {
        let path = Path::new(argument);
        if settings.sandbox.enabled && path.is_absolute() && path.is_file()
            && (is_wine_runtime(path) || path.canonicalize().is_ok_and(|resolved| is_wine_runtime(&resolved))) {
            argv.push(sandbox_program(path.to_path_buf(), &mut sandbox_settings)?);
        } else { argv.push(argument.clone()); }
    }
    argv.extend(settings.arguments.iter().map(std::ffi::OsString::from));
    let program = Path::new(&argv[0]);
    let mut command = if settings.sandbox.enabled {
        if probe { probe_sandbox(game, &sandbox_settings)?; }
        super::sandbox::command(game, &sandbox_settings, program)?
    } else {
        let mut command = Command::new(program);
        command.current_dir(settings.working_directory.as_deref().unwrap_or(install));
        // Store authentication belongs to the launcher, not a direct game.
        command.env_remove("MODDE_STEAM_API_KEY");
        // A direct Wine/umu launch must not inherit a different Proton save
        // destination from the terminal or launcher that opened modde. Store
        // boundaries explicitly forward and validate these variables instead.
        command.env_remove("WINEPREFIX").env_remove("STEAM_COMPAT_DATA_PATH")
            .env_remove("UMU_CONTAINER_NSENTER");
        command.envs(&settings.environment);
        let runtime = super::runtime::RuntimePaths::resolve(settings)?;
        command.env("HOME", runtime.home).env("XDG_CONFIG_HOME", runtime.config)
            .env("XDG_DATA_HOME", runtime.data).env("XDG_CACHE_HOME", runtime.cache);
        if let Some(prefix) = wine_environment_prefix(settings)? { command.env("WINEPREFIX", prefix); }
        command
    };
    command.args(&argv[1..]);
    Ok(command)
}

fn is_wine_runtime(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| matches!(name, "wine" | "wine64" | "wineserver" | "proton" | "umu-run"))
}

fn sandbox_program(path: PathBuf, settings: &mut LaunchSettings) -> Result<std::ffi::OsString> {
    if !settings.sandbox.enabled { return Ok(path.into_os_string()); }
    // PATH programs may be private-HOME symlinks. Grant the resolved file, not
    // its arbitrary parent: ~/launch.sh must not expose the rest of ~/.
    let resolved = path.canonicalize().with_context(|| format!("resolving launch program {}", path.display()))?;
    let runtime = is_wine_runtime(&path) || is_wine_runtime(&resolved);
    settings.sandbox.read_only.push(resolved.clone());
    if runtime && let Some(parent) = resolved.parent() {
        let root = if parent.file_name().is_some_and(|name| name == "bin") { parent.parent().unwrap_or(parent) } else { parent };
        let logical = super::runtime::RuntimePaths::resolve(settings)?;
        let host = super::runtime::RuntimePaths::host();
        // Only infer dedicated runner trees. An unusual runner installed at a
        // user root can still receive explicit grants through Launch Settings.
        let user_roots = [&logical.home, &logical.config, &logical.data, &logical.cache,
            &host.home, &host.config, &host.data, &host.cache];
        if root.parent().is_some() && !user_roots.iter().any(|path| modde_core::library::normalized_path(path).starts_with(root)) {
            settings.sandbox.read_only.push(root.to_path_buf());
        }
    }
    Ok(resolved.into_os_string())
}

pub fn preflight(game: &LibraryGame, settings: &LaunchSettings, games: &[LibraryGame]) -> Result<()> {
    validate(game, settings, games)?;
    if settings.sandbox.enabled {
        probe_sandbox(game, settings)?;
    }
    Ok(())
}

fn probe_sandbox(game: &LibraryGame, settings: &LaunchSettings) -> Result<()> {
    // Resolve independently of a game's custom PATH, and grant the real target
    // so a private-HOME symlink cannot make only the probe fail.
    let program = resolve_wrapper("true", &LaunchSettings::default())?.canonicalize()?;
    let mut probe = settings.clone();
    probe.sandbox.read_only.push(program.clone());
    let status = super::sandbox::command(game, &probe, &program)?
        .status().context("bubblewrap preflight failed (install bwrap and enable user namespaces)")?;
    if !status.success() { bail!("bubblewrap preflight exited with {status}; game was not launched"); }
    Ok(())
}

pub fn install(game: &LibraryGame) -> Result<()> {
    open::that(store_uri(game, true)?).context("opening store installation page")?;
    Ok(())
}

fn uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) { encoded.push(char::from(byte)); }
        else { use std::fmt::Write; let _ = write!(encoded, "%{byte:02X}"); }
    }
    encoded
}

pub fn store_uri(game: &LibraryGame, install: bool) -> Result<String> {
    if game.app_id.is_empty() { bail!("store app ID is empty"); }
    match game.store {
        Store::Steam => {
            if game.app_id.is_empty() || !game.app_id.bytes().all(|byte| byte.is_ascii_digit()) { bail!("invalid Steam app ID"); }
            Ok(format!("steam://{}/{}", if install { "install" } else { "rungameid" }, game.app_id))
        }
        Store::Gog | Store::Epic | Store::Sideload => {
            let runner = match game.store { Store::Gog => "gog", Store::Epic => "legendary", _ => "sideload" };
            // Heroic's launch protocol opens its installation prompt for an uninstalled title.
            Ok(format!("heroic://launch?appName={}&runner={runner}", uri_component(&game.app_id)))
        }
        Store::Local => bail!("local games have no store installation action"),
    }
}

/// Resource keys cover physical aliases and shared prefixes/save directories.
pub fn lock_paths(game: &LibraryGame, settings: &LaunchSettings) -> Vec<PathBuf> {
    let root = modde_core::paths::modde_config_dir().join("sessions");
    let mut keys = vec![game.id.clone()];
    for path in [game.install_path.as_ref(), settings.prefix.as_ref(), settings.save_directory.as_ref()].into_iter().flatten() {
        let path = modde_core::library::normalized_path(path);
        keys.push(modde_core::library::installation_id("resource", &path));
    }
    keys.sort();
    keys.dedup();
    keys.into_iter().map(|key| root.join(format!("{key}.lock"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_sandbox_never_falls_back_to_a_uri() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(dir.path().into()));
        let mut settings = LaunchSettings::default();
        settings.sandbox.enabled = true;
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).unwrap_err().to_string().contains("cannot be sandboxed"));
    }

    #[test]
    fn store_hook_accepts_sandbox_settings_but_requires_one_installation() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(a.path().into()));
        let second = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(b.path().into()));
        let mut settings = LaunchSettings { store_hook: true, ..Default::default() };
        settings.sandbox.enabled = cfg!(target_os = "linux");
        assert!(validate(&first, &settings, std::slice::from_ref(&first)).is_ok());
        assert!(validate(&first, &settings, &[first.clone(), second]).is_err());
    }

    #[test]
    fn store_boundary_retains_non_shell_arguments_and_appends_saved_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(dir.path().into()));
        let settings = LaunchSettings { arguments: vec!["saved value".into()], store_hook: true, ..Default::default() };
        let supplied = vec![dir.path().join("runtime launcher").into_os_string(), "--".into(), "$(not-a-shell)".into(), "game path.exe".into()];
        let command = boundary_command(&game, &settings, &supplied).unwrap();
        let expected: Vec<_> = supplied[1..].iter().cloned().chain(["saved value".into()]).collect();
        assert_eq!(command.get_args().map(std::ffi::OsString::from).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn ambiguous_store_launch_is_rejected_before_preparation() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(a.path().into()));
        let second = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(b.path().into()));
        assert!(validate(&first, &LaunchSettings::default(), &[first.clone(), second]).is_err());
    }

    #[test]
    fn direct_arguments_are_not_shell_interpreted() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Local, "custom".into(), "Custom".into(), Some(dir.path().into()));
        let settings = LaunchSettings {
            executable: Some(dir.path().join("game with spaces")),
            arguments: vec!["$(touch /bad)".into(), "a b".into()],
            ..LaunchSettings::default()
        };
        let command = direct_command(&game, &settings).unwrap();
        assert_eq!(command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["$(touch /bad)", "a b"]);
    }

    #[test]
    fn direct_launch_clears_ambient_prefixes_and_preserves_explicit_runtime_settings() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Local, "example".into(), "Example".into(), Some(dir.path().into()));
        let mut settings = LaunchSettings { executable: Some(dir.path().join("game")), ..Default::default() };
        let command = direct_command(&game, &settings).unwrap();
        for key in ["WINEPREFIX", "STEAM_COMPAT_DATA_PATH", "UMU_CONTAINER_NSENTER"] {
            assert!(command.get_envs().any(|(name, value)| name == key && value.is_none()));
        }
        let compat = dir.path().join("compat");
        settings.prefix = Some(compat.join("pfx"));
        settings.environment.insert("STEAM_COMPAT_DATA_PATH".into(), compat.to_string_lossy().into_owned());
        let command = direct_command(&game, &settings).unwrap();
        assert!(command.get_envs().any(|(name, value)| name == "WINEPREFIX" && value == settings.prefix.as_deref().map(Path::as_os_str)));
        assert!(command.get_envs().any(|(name, value)| name == "STEAM_COMPAT_DATA_PATH" && value == Some(compat.as_os_str())));
    }

    #[cfg(unix)]
    #[test]
    fn missing_generated_wrapper_is_rejected_during_validation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("game");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let game = LibraryGame::new(Store::Local, "example".into(), "Example".into(), Some(dir.path().into()));
        let settings = LaunchSettings {
            executable: Some(executable),
            wrappers: vec![vec![dir.path().join("missing-wrapper").to_string_lossy().into_owned()]],
            ..Default::default()
        };
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_err());
    }

    #[test]
    fn steam_compat_environment_cannot_redirect_saves_to_another_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(dir.path().into()));
        let mut settings = LaunchSettings { store_hook: true, prefix: Some(dir.path().join("compat/pfx")), ..Default::default() };
        settings.environment.insert("STEAM_COMPAT_DATA_PATH".into(), dir.path().join("other").to_string_lossy().into_owned());
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_err());
        settings.environment.insert("STEAM_COMPAT_DATA_PATH".into(), dir.path().join("compat").to_string_lossy().into_owned());
        // A first launch can create the prefix after path validation.
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_ok());
    }

    #[test]
    fn a_proton_container_prefix_is_preserved_for_the_runner() {
        let root = tempfile::tempdir().unwrap();
        let compat = root.path().join("compat");
        let prefix = compat.join("pfx");
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        // An old Wine root must not override the existing raw-Proton pfx.
        std::fs::create_dir(compat.join("drive_c")).unwrap();
        let game = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(root.path().into()));
        let mut settings = LaunchSettings { store_hook: true, prefix: Some(prefix.clone()), ..Default::default() };
        settings.environment.insert("STEAM_COMPAT_DATA_PATH".into(), compat.to_string_lossy().into_owned());
        settings.environment.insert("WINEPREFIX".into(), compat.to_string_lossy().into_owned());
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_ok());
        // Supply an absolute command for command construction only; no runner
        // or process is executed by this regression.
        let command = boundary_command(&game, &settings, &[root.path().join("umu-run").into_os_string()]).unwrap();
        assert!(command.get_envs().any(|(name, value)| name == "WINEPREFIX" && value == Some(compat.as_os_str())));
        assert_eq!(settings.prefix, Some(prefix.clone()));
        settings.environment.insert("WINEPREFIX".into(), root.path().join("other").to_string_lossy().into_owned());
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_err());
        settings.environment.insert("WINEPREFIX".into(), compat.to_string_lossy().into_owned());
        settings.environment.remove("STEAM_COMPAT_DATA_PATH");
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_err());
    }

    #[test]
    fn a_reused_umu_service_is_rejected_before_preparation() {
        let root = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(Store::Steam, "1".into(), "A".into(), Some(root.path().into()));
        let mut settings = LaunchSettings { store_hook: true, ..Default::default() };
        settings.environment.insert("UMU_CONTAINER_NSENTER".into(), "1".into());
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).unwrap_err().to_string().contains("existing UMU service"));
        settings.environment.insert("UMU_CONTAINER_NSENTER".into(), "0".into());
        assert!(validate(&game, &settings, std::slice::from_ref(&game)).is_ok());
    }
}
