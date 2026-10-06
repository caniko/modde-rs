//! Stable render-device selection for game processes, independent of media decode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use modde_core::library::LaunchSettings;
use serde::{Deserialize, Serialize};

/// Variables which could redirect a typed Mesa selection to another device.
pub(crate) const SELECTION_KEYS: &[&str] = &[
    "DRI_PRIME",
    "MESA_VK_DEVICE_SELECT",
    "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__NV_PRIME_RENDER_OFFLOAD",
    "__NV_PRIME_RENDER_OFFLOAD_PROVIDER",
    "__VK_LAYER_NV_optimus",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub node: PathBuf,
    pub pci: String,
    pub vendor_id: String,
    pub device_id: String,
    pub driver: String,
    pub driver_version: Option<String>,
}

/// Requested routing and host inventory, not proof of a game's actual renderer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub requested_node: Option<PathBuf>,
    pub selected: Option<Device>,
    pub available: Vec<Device>,
    pub environment: BTreeMap<String, String>,
    pub kernel: Option<String>,
    pub graphics_driver_root: Option<PathBuf>,
    pub graphics_driver_32_root: Option<PathBuf>,
}

fn selected_node(settings: &LaunchSettings, host: Option<&Path>) -> Option<PathBuf> {
    if let Some(saved) = &settings.gpu_render_node {
        Some(saved.clone())
    } else if SELECTION_KEYS
        .iter()
        .any(|key| settings.environment.contains_key(*key))
    {
        // Explicit launch-environment selection is also an installation choice.
        None
    } else {
        host.map(Path::to_path_buf)
    }
}

pub fn configured_node(settings: &LaunchSettings) -> Option<PathBuf> {
    let host = std::env::var_os("MODDE_GPU_RENDER_NODE").map(PathBuf::from);
    selected_node(settings, host.as_deref())
}

/// Mesa documents PCI selection for both OpenGL and Vulkan:
/// <https://docs.mesa3d.org/envvars.html#dri-prime>
fn pci_selector(path: &Path) -> Result<String> {
    ensure!(
        path.parent() == Some(Path::new("/dev/dri/by-path")),
        "GPU selection requires a stable /dev/dri/by-path/pci-...-render node"
    );
    let pci = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("pci-"))
        .and_then(|s| s.strip_suffix("-render"))
        .context("GPU selection requires a PCI render-node alias")?;
    let bytes = pci.as_bytes();
    ensure!(
        bytes.len() == 12
            && bytes[4] == b':'
            && bytes[7] == b':'
            && bytes[10] == b'.'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| matches!(i, 4 | 7 | 10) || b.is_ascii_hexdigit())
            && bytes[11] <= b'7',
        "invalid GPU PCI identity"
    );
    Ok(format!(
        "pci-{}",
        pci.replace([':', '.'], "_").to_ascii_lowercase()
    ))
}

#[cfg(target_os = "linux")]
fn device(node: &Path) -> Result<Device> {
    use std::os::unix::fs::FileTypeExt;
    let node = node
        .canonicalize()
        .with_context(|| format!("GPU node is unavailable: {}", node.display()))?;
    ensure!(
        node.parent() == Some(Path::new("/dev/dri"))
            && node.file_name().and_then(|s| s.to_str()).is_some_and(|s| s
                .strip_prefix("renderD")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
            && node.metadata()?.file_type().is_char_device(),
        "GPU selection must resolve to a DRM render character device"
    );
    let name = node.file_name().context("GPU node name missing")?;
    let hardware = Path::new("/sys/class/drm")
        .join(name)
        .join("device")
        .canonicalize()?;
    let pci = hardware
        .file_name()
        .and_then(|s| s.to_str())
        .context("GPU PCI identity unavailable")?
        .to_owned();
    let driver = hardware
        .join("driver")
        .canonicalize()?
        .file_name()
        .and_then(|s| s.to_str())
        .context("GPU driver unavailable")?
        .to_owned();
    let read = |name| -> Result<String> {
        Ok(std::fs::read_to_string(hardware.join(name))?
            .trim()
            .to_owned())
    };
    Ok(Device {
        node,
        pci,
        vendor_id: read("vendor")?,
        device_id: read("device")?,
        driver_version: std::fs::read_to_string(hardware.join("driver/module/version"))
            .ok()
            .map(|s| s.trim().to_owned()),
        driver,
    })
}

#[cfg(not(target_os = "linux"))]
fn device(_node: &Path) -> Result<Device> {
    bail!("explicit render-node selection requires Linux")
}

pub fn resolve(settings: &LaunchSettings) -> Result<Option<Device>> {
    let Some(node) = configured_node(settings) else {
        return Ok(None);
    };
    let selector = pci_selector(&node)?;
    let selected = device(&node)?;
    ensure!(
        selector == format!("pci-{}", selected.pci.replace([':', '.'], "_")),
        "GPU alias resolves to a different PCI device"
    );
    ensure!(
        matches!(
            selected.driver.as_str(),
            "amdgpu" | "radeon" | "i915" | "xe"
        ),
        "explicit render-node selection requires an AMD/Intel Mesa driver; use launch environment settings for {}",
        selected.driver
    );
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&selected.node)
        .context("GPU render node is not accessible to this user")?;
    selection_environment(settings, &selector)?;
    Ok(Some(selected))
}

fn selection_environment(
    settings: &LaunchSettings,
    selector: &str,
) -> Result<BTreeMap<String, String>> {
    for key in SELECTION_KEYS {
        if let Some(value) = settings.environment.get(*key)
            && !(*key == "DRI_PRIME" && value == selector)
        {
            bail!("{key} conflicts with gpu_render_node; use one GPU selection mechanism");
        }
    }
    Ok(BTreeMap::from([("DRI_PRIME".into(), selector.into())]))
}

pub(crate) fn configure(settings: &mut LaunchSettings) -> Result<()> {
    if resolve(settings)?.is_some() {
        let node = configured_node(settings).context("GPU selection disappeared")?;
        settings
            .environment
            .extend(selection_environment(settings, &pci_selector(&node)?)?);
        settings.gpu_render_node = Some(node);
    }
    Ok(())
}

pub fn snapshot(settings: &LaunchSettings) -> Result<Snapshot> {
    let selected = resolve(settings)?;
    let mut available: Vec<_> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| device(&entry.path()).ok())
        .collect();
    available.sort_by(|a, b| a.pci.cmp(&b.pci));
    let mut environment = BTreeMap::new();
    for key in SELECTION_KEYS.iter().copied().chain([
        "VK_DRIVER_FILES",
        "VK_ICD_FILENAMES",
        "LIBGL_ALWAYS_SOFTWARE",
        "LIBGL_DRIVERS_PATH",
        "MESA_LOADER_DRIVER_OVERRIDE",
    ]) {
        if let Some(value) = settings
            .environment
            .get(key)
            .cloned()
            .or_else(|| std::env::var(key).ok())
        {
            environment.insert(key.into(), value);
        }
    }
    if let Some(node) = configured_node(settings) {
        for key in SELECTION_KEYS {
            environment.remove(*key);
        }
        environment.extend(selection_environment(settings, &pci_selector(&node)?)?);
    }
    Ok(Snapshot {
        requested_node: configured_node(settings),
        selected,
        available,
        environment,
        kernel: std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|s| s.trim().to_owned()),
        graphics_driver_root: Path::new("/run/opengl-driver").canonicalize().ok(),
        graphics_driver_32_root: Path::new("/run/opengl-driver-32").canonicalize().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_pci_identity_survives_render_node_renumbering() {
        assert_eq!(
            pci_selector(Path::new("/dev/dri/by-path/pci-0000:03:00.0-render")).unwrap(),
            "pci-0000_03_00_0"
        );
        for path in [
            "/dev/dri/renderD128",
            "/dev/dri/by-path/../renderD128",
            "/dev/dri/by-path/pci-0000:03:00.0-card",
        ] {
            assert!(pci_selector(Path::new(path)).is_err(), "{path}");
        }
    }

    #[test]
    fn installation_choice_wins_over_host_default() {
        let saved = Path::new("/dev/dri/by-path/pci-0000:03:00.0-render");
        let host = Path::new("/dev/dri/by-path/pci-0000:65:00.0-render");
        let mut settings = LaunchSettings {
            gpu_render_node: Some(saved.into()),
            ..Default::default()
        };
        assert_eq!(selected_node(&settings, Some(host)), Some(saved.to_owned()));
        settings.gpu_render_node = None;
        assert_eq!(selected_node(&settings, Some(host)), Some(host.to_owned()));
        assert_eq!(selected_node(&settings, None), None);
        settings.environment.insert("DRI_PRIME".into(), "0".into());
        assert_eq!(selected_node(&settings, Some(host)), None);
    }

    #[test]
    fn explicit_choice_rejects_conflicting_manual_selection() {
        let mut settings = modde_core::library::LaunchSettings::default();
        settings.environment.insert("DRI_PRIME".into(), "1".into());
        assert!(selection_environment(&settings, "pci-0000_03_00_0").is_err());
        settings.environment.clear();
        assert_eq!(
            selection_environment(&settings, "pci-0000_03_00_0").unwrap()["DRI_PRIME"],
            "pci-0000_03_00_0"
        );
    }

    #[test]
    fn missing_selected_device_cannot_become_a_desktop_fallback() {
        let settings = LaunchSettings {
            gpu_render_node: Some("/dev/dri/by-path/pci-ffff:ff:ff.7-render".into()),
            ..Default::default()
        };
        assert!(resolve(&settings).is_err());
    }
}
