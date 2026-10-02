//! A filesystem sandbox around the actual executable. Bind mounts and native
//! GPU devices avoid copying assets or interposing on rendering/file reads.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use modde_core::library::LaunchSettings;

use super::LibraryGame;

#[cfg(target_os = "linux")]
pub(super) const SYSTEM_READ_ONLY: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/nix/store",
    "/sys",
    "/app",
    "/overrides",
    "/.flatpak-info",
    "/run/current-system/sw",
    "/run/opengl-driver",
    "/run/opengl-driver-32",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/fonts",
    "/etc/ssl/certs",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/group",
    "/etc/localtime",
    "/etc/vulkan",
    "/etc/glvnd",
];

/// Existing directory grants preserve their symlinks. Mounting a file over such
/// a symlink is rejected by bubblewrap; aliases hidden by private HOME need a
/// file-only mount instead. The resolved target is always granted separately.
pub(super) fn program_alias_is_visible(
    game: &LibraryGame,
    settings: &LaunchSettings,
    path: &Path,
) -> bool {
    #[cfg(target_os = "linux")]
    if SYSTEM_READ_ONLY
        .iter()
        .map(Path::new)
        .any(|root| root.is_dir() && path.starts_with(root))
    {
        return true;
    }
    game.install_path
        .iter()
        .chain(settings.prefix.iter())
        .chain(settings.save_directory.iter())
        .chain(settings.sandbox.read_only.iter())
        .chain(settings.sandbox.writable.iter())
        .any(|root| root != path && root.is_dir() && path.starts_with(root))
}

pub(super) fn validate(settings: &LaunchSettings) -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("bubblewrap sandboxing is available on Linux only");
    }
    for path in settings
        .sandbox
        .read_only
        .iter()
        .chain(&settings.sandbox.writable)
    {
        if !path.is_absolute() || !path.exists() {
            bail!(
                "sandbox mount must be an existing absolute path: {}",
                path.display()
            );
        }
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!("sandbox mount must not contain '..'");
        }
        if path.canonicalize()?.parent().is_none() {
            bail!("sandbox cannot expose the host root");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn command(
    _game: &LibraryGame,
    _settings: &LaunchSettings,
    _program: &Path,
) -> Result<Command> {
    bail!("bubblewrap sandboxing is available on Linux only")
}

#[cfg(target_os = "linux")]
pub(super) fn command(
    game: &LibraryGame,
    settings: &LaunchSettings,
    program: &Path,
) -> Result<Command> {
    use anyhow::Context;
    use modde_core::paths;

    validate(settings)?;
    let install = game
        .install_path
        .as_deref()
        .context("sandbox requires an installation")?;
    for path in [
        Some(install),
        settings.prefix.as_deref(),
        settings.save_directory.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if path.canonicalize()?.parent().is_none() {
            bail!("sandbox cannot expose the host root");
        }
    }
    let runtime = paths::modde_data_dir().join("runtime").join(&game.id);
    let logical = super::runtime::RuntimePaths::resolve(settings)?;
    let mut command = Command::new("bwrap");
    command.args([
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--cap-drop",
        "ALL",
        "--clearenv",
    ]);
    if !settings.sandbox.network {
        command.arg("--unshare-net");
    }
    command.args([
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/dev/shm", "--tmpfs", "/tmp",
    ]);
    for path in SYSTEM_READ_ONLY {
        bind_if_exists(&mut command, "--ro-bind", Path::new(path));
    }
    for path in [
        "/dev/dri",
        "/dev/snd",
        "/dev/input",
        "/dev/nvidia0",
        "/dev/nvidia1",
        "/dev/nvidiactl",
        "/dev/nvidia-modeset",
        "/dev/nvidia-uvm",
        "/dev/nvidia-uvm-tools",
    ] {
        bind_if_exists(&mut command, "--dev-bind", Path::new(path));
    }
    // Preserve the logical HOME so native save paths and Wine's shell-folder
    // symlinks still resolve. Its contents are private; individual granted
    // directories below are mounted over it afterwards.
    let home_backing = runtime.join("home");
    let backing = |path: &Path, name: &str| {
        path.strip_prefix(&logical.home).map_or_else(
            |_| runtime.join(name),
            |relative| home_backing.join(relative),
        )
    };
    let mut private = vec![
        (&logical.home, home_backing.clone()),
        (&logical.config, backing(&logical.config, "config")),
        (&logical.data, backing(&logical.data, "data")),
        (&logical.cache, runtime.join("cache")),
    ];
    private.sort_by(|(a, _), (b, _)| {
        a.components()
            .count()
            .cmp(&b.components().count())
            .then_with(|| a.cmp(b))
    });
    private.dedup_by(|(a, _), (b, _)| a == b);
    for (location, backing) in private {
        std::fs::create_dir_all(&backing)?;
        command.arg("--bind").arg(backing).arg(location);
    }
    // Only display/audio sockets, never the user's entire runtime directory or
    // session bus. A connected X11 server has its own weaker isolation model.
    bind_if_exists(&mut command, "--ro-bind", Path::new("/tmp/.X11-unix"));
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let runtime_dir = std::path::PathBuf::from(runtime_dir);
        if runtime_dir.is_absolute() {
            command.arg("--dir").arg(&runtime_dir);
            if let Ok(display) = std::env::var("WAYLAND_DISPLAY") {
                let socket = if Path::new(&display).is_absolute() {
                    display.into()
                } else {
                    runtime_dir.join(display)
                };
                bind_if_exists(&mut command, "--ro-bind", &socket);
            }
            bind_if_exists(&mut command, "--ro-bind", &runtime_dir.join("pulse/native"));
            bind_if_exists(&mut command, "--ro-bind", &runtime_dir.join("pipewire-0"));
        }
    }
    if let Some(authority) = std::env::var_os("XAUTHORITY") {
        let authority = std::path::PathBuf::from(authority);
        if authority.is_absolute() {
            bind_if_exists(&mut command, "--ro-bind", &authority);
        }
    }
    bind_if_exists(
        &mut command,
        "--ro-bind",
        &paths::user_config_dir().join("pulse/cookie"),
    );
    // Deployed mods may symlink into these trees; exposing only the game root
    // would produce missing assets while the unsandboxed game worked.
    for path in [
        paths::store_dir(),
        paths::staging_dir(),
        paths::generated_dir(),
        paths::profiles_dir(),
        paths::modde_data_dir().join("tools"),
    ] {
        // Preparation precedes deployment. Mount stable parent directories even
        // on the first run, so files materialized later are visible to the game.
        std::fs::create_dir_all(&path)?;
        bind(&mut command, "--ro-bind", &path);
    }
    for path in &settings.sandbox.read_only {
        bind(&mut command, "--ro-bind", path);
    }
    if let Some(proton) = settings.environment.get("PROTONPATH").map(Path::new)
        && proton.is_absolute()
        && proton.is_dir()
    {
        if proton.canonicalize()?.parent().is_none() {
            bail!("Proton runtime cannot expose the host root");
        }
        bind(&mut command, "--ro-bind", proton);
    }
    bind(&mut command, "--bind", install);
    // Proton/UMU write metadata next to pfx as well as saves inside it. The
    // validated compat root still targets this installation's physical prefix.
    super::launch::wine_environment_prefix(settings)?;
    if let Some(compat) = settings
        .environment
        .get("STEAM_COMPAT_DATA_PATH")
        .map(Path::new)
    {
        if !compat.is_dir() {
            bail!("sandbox compat directory is missing: {}", compat.display());
        }
        bind(&mut command, "--bind", compat);
    }
    for path in [
        settings.prefix.as_deref(),
        settings.save_directory.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !path.is_absolute() || !path.is_dir() {
            bail!("sandbox writable directory is missing: {}", path.display());
        }
        bind(&mut command, "--bind", path);
    }
    for path in &settings.sandbox.writable {
        bind(&mut command, "--bind", path);
    }
    // Allowlist prevents API credentials and unrelated inherited secrets from
    // entering the game. User-specified overrides are intentional grants.
    for key in [
        "PATH",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TZ",
        "USER",
        "LOGNAME",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XAUTHORITY",
        "PULSE_SERVER",
        "LD_LIBRARY_PATH",
        "LIBGL_DRIVERS_PATH",
        "VK_ICD_FILENAMES",
        "VK_DRIVER_FILES",
        "__GLX_VENDOR_LIBRARY_NAME",
        "__NV_PRIME_RENDER_OFFLOAD",
        "DRI_PRIME",
        "MESA_VK_DEVICE_SELECT",
        "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE",
        "__NV_PRIME_RENDER_OFFLOAD_PROVIDER",
        "__VK_LAYER_NV_optimus",
        "LIBGL_ALWAYS_SOFTWARE",
        "MESA_LOADER_DRIVER_OVERRIDE",
    ] {
        if settings.gpu_render_node.is_some() && super::gpu::SELECTION_KEYS.contains(&key) {
            continue;
        }
        if let Some(value) = std::env::var_os(key) {
            command.args(["--setenv", key]).arg(value);
        }
    }
    for (key, value) in &settings.environment {
        command.args(["--setenv", key, value]);
    }
    for (key, path) in [
        ("HOME", &logical.home),
        ("XDG_CONFIG_HOME", &logical.config),
        ("XDG_DATA_HOME", &logical.data),
        ("XDG_CACHE_HOME", &logical.cache),
    ] {
        command.args(["--setenv", key]).arg(path);
    }
    if let Some(prefix) = super::launch::wine_environment_prefix(settings)? {
        command.args(["--setenv", "WINEPREFIX"]).arg(prefix);
    }
    command
        .arg("--chdir")
        .arg(settings.working_directory.as_deref().unwrap_or(install));
    command.arg("--").arg(program);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn bind(command: &mut Command, mode: &str, path: &Path) {
    command.arg(mode).arg(path).arg(path);
}

#[cfg(target_os = "linux")]
fn bind_if_exists(command: &mut Command, mode: &str, path: &Path) {
    if path.exists() {
        bind(command, mode, path);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_symlink_cannot_turn_an_extra_mount_into_the_host_root() {
        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("root-alias");
        std::os::unix::fs::symlink("/", &alias).unwrap();
        let mut settings = LaunchSettings::default();
        settings.sandbox.read_only.push(alias);
        assert!(
            validate(&settings)
                .unwrap_err()
                .to_string()
                .contains("host root")
        );
    }

    #[test]
    fn an_automatic_prefix_mount_cannot_expose_the_host_root() {
        let dir = tempfile::tempdir().unwrap();
        let game = LibraryGame::new(
            super::super::Store::Local,
            "test".into(),
            "Test".into(),
            Some(dir.path().into()),
        );
        let settings = LaunchSettings {
            prefix: Some("/".into()),
            ..LaunchSettings::default()
        };
        assert!(
            command(&game, &settings, Path::new("true"))
                .unwrap_err()
                .to_string()
                .contains("host root")
        );
    }
}
