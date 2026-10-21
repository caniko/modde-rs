#![allow(clippy::wildcard_imports)]
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use modde_core::{NexusFileId, NexusModId};
use tracing::info;

/// Parsed nxm:// URI.
///
/// Format: `nxm://<game_domain>/mods/<mod_id>/files/<file_id>?key=<key>&expires=<ts>`
#[derive(Debug, Clone)]
pub struct NxmUri {
    pub game_domain: String,
    pub mod_id: NexusModId,
    pub file_id: NexusFileId,
    /// Authorization key from a premium nxm:// link. Parsed for a complete
    /// representation of the URI; not yet consumed by the download flow.
    pub key: Option<String>,
    /// Expiry timestamp from a premium nxm:// link (see `key`).
    pub expires: Option<u64>,
}

impl NxmUri {
    /// Parse an nxm:// URI string.
    pub fn parse(uri: &str) -> Result<Self> {
        let uri = uri
            .strip_prefix("nxm://")
            .ok_or_else(|| anyhow::anyhow!("not an nxm:// URI: {uri}"))?;

        // Split off query string
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));

        // Expected: [game_domain, "mods", mod_id, "files", file_id]
        let segments = path.split('/').collect::<Vec<_>>();
        let [game_domain, "mods", mod_id, "files", file_id] = segments.as_slice() else {
            bail!("invalid nxm:// URI format: nxm://{uri}");
        };
        if game_domain.is_empty() || mod_id.is_empty() || file_id.is_empty() {
            bail!("invalid nxm:// URI format: nxm://{uri}");
        }

        modde_core::nexus_id::validate_game_domain(game_domain)?;
        let game_domain = game_domain.to_string();
        let mod_id = mod_id
            .parse::<u64>()
            .map(NexusModId::from)
            .with_context(|| format!("invalid mod_id: {mod_id}"))?;
        let file_id = file_id
            .parse::<u64>()
            .map(NexusFileId::from)
            .with_context(|| format!("invalid file_id: {file_id}"))?;

        // Parse query parameters
        let mut key = None;
        let mut expires = None;
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                match k {
                    "key" => key = Some(v.to_string()),
                    "expires" => expires = v.parse().ok(),
                    _ => {}
                }
            }
        }

        Ok(NxmUri {
            game_domain,
            mod_id,
            file_id,
            key,
            expires,
        })
    }
}

/// Handle an nxm:// URI — download the mod file.
pub async fn handle(uri: String, _profile: Option<String>) -> Result<()> {
    let parsed = NxmUri::parse(&uri)?;
    if parsed.key.is_some() || parsed.expires.is_some() {
        info!(
            has_key = parsed.key.is_some(),
            expires = parsed.expires,
            "nxm URI included premium metadata; resolving download through configured API key"
        );
    }

    println!("nxm:// download request:");
    println!("  Game: {}", parsed.game_domain);
    println!("  Mod ID: {}", parsed.mod_id);
    println!("  File ID: {}", parsed.file_id);

    let api_key = modde_sources::nexus::auth::load_api_key()?;
    let client = reqwest::Client::new();

    // Get mod info
    let api = modde_sources::nexus::api::NexusApi::new(client.clone(), api_key.clone());
    let mod_info = api.get_mod(&parsed.game_domain, parsed.mod_id).await?;

    println!("  Mod: {} by {}", mod_info.name, mod_info.author);
    println!("  Version: {}", mod_info.version);

    // Generate download link
    let download_url = modde_sources::nexus::cdn::generate_download_link(
        &client,
        &api_key,
        &parsed.game_domain,
        parsed.mod_id,
        parsed.file_id,
    )
    .await?;

    info!(url = %download_url, "resolved CDN URL");

    // Download to the downloads directory
    let downloads_dir = modde_core::paths::downloads_dir();
    std::fs::create_dir_all(&downloads_dir)?;

    let file_info = api
        .get_mod_files(&parsed.game_domain, parsed.mod_id)
        .await?;
    let file_name = file_info
        .files
        .iter()
        .find(|f| f.file_id == parsed.file_id)
        .map_or_else(
            || format!("{}_{}.zip", parsed.mod_id, parsed.file_id),
            |f| f.file_name.clone(),
        );

    let dest = downloads_dir.join(&file_name);
    println!("  Downloading to: {}", dest.display());

    let resp = client.get(&download_url).send().await?;
    let bytes = resp.bytes().await?;
    tokio::fs::write(&dest, &bytes).await?;

    println!("  Download complete: {}", dest.display());
    println!("\nInstall with: modde install mod '{}'", dest.display());

    Ok(())
}

/// Install the platform-appropriate handler for nxm:// URIs.
///
/// - Linux: XDG desktop file + `xdg-mime` registration
/// - macOS: `.app` bundle with `CFBundleURLTypes` in `Info.plist`
/// - Windows: Registry-based protocol handler
pub fn install_handler() -> Result<PathBuf> {
    install_handler_platform()
}

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn install_handler_platform() -> Result<PathBuf> {
    let desktop_entry = r"[Desktop Entry]
Type=Application
Name=modde NXM Handler
Comment=Handle nxm:// download links from Nexus Mods
Exec=modde nxm handle %u
Terminal=false
NoDisplay=true
MimeType=x-scheme-handler/nxm;
Categories=Game;
";

    let home = modde_core::paths::home_dir();
    let desktop_dir = home.join(".local/share/applications");
    std::fs::create_dir_all(&desktop_dir)?;

    let desktop_path = desktop_dir.join("modde-nxm-handler.desktop");
    std::fs::write(&desktop_path, desktop_entry)?;

    println!("Installed NXM handler: {}", desktop_path.display());
    println!("\nRegistering with xdg-mime...");

    let status = std::process::Command::new("xdg-mime")
        .args([
            "default",
            "modde-nxm-handler.desktop",
            "x-scheme-handler/nxm",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("Registered as default nxm:// handler.");
            println!("You can now click 'Download with Mod Manager' on Nexus Mods.");
        }
        _ => {
            println!("Warning: xdg-mime registration failed.");
            println!("Manually register with:");
            println!("  xdg-mime default modde-nxm-handler.desktop x-scheme-handler/nxm");
        }
    }

    Ok(desktop_path)
}

#[cfg(all(target_os = "macos", feature = "macos-integrations"))]
fn install_handler_platform() -> Result<PathBuf> {
    let exe_path = std::env::current_exe().context("failed to determine modde binary path")?;
    let home = modde_core::paths::home_dir();

    let app_dir = home.join("Applications/modde-nxm-handler.app/Contents");
    let macos_dir = app_dir.join("MacOS");
    std::fs::create_dir_all(&macos_dir)?;

    // Write Info.plist with URL scheme registration
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>modde NXM Handler</string>
    <key>CFBundleIdentifier</key>
    <string>com.modde.nxm-handler</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleExecutable</key>
    <string>nxm-handler</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key>
            <string>NXM Protocol</string>
            <key>CFBundleURLSchemes</key>
            <array>
                <string>nxm</string>
            </array>
        </dict>
    </array>
</dict>
</plist>"#
    );
    std::fs::write(app_dir.join("Info.plist"), plist)?;

    // Write a small shell script that forwards to the real binary
    let handler_script = format!(
        "#!/bin/sh\nexec \"{}\" nxm handle \"$@\"\n",
        exe_path.display()
    );
    let handler_path = macos_dir.join("nxm-handler");
    std::fs::write(&handler_path, handler_script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&handler_path, std::fs::Permissions::from_mode(0o755))?;
    }

    let app_path = home.join("Applications/modde-nxm-handler.app");
    println!("Installed NXM handler: {}", app_path.display());
    println!("The nxm:// protocol should be registered automatically.");
    println!("If not, open the .app once to register it with Launch Services.");

    Ok(app_path)
}

#[cfg(all(target_os = "windows", feature = "windows-integrations"))]
fn install_handler_platform() -> Result<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::*;

    let exe_path = std::env::current_exe().context("failed to determine modde binary path")?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Create nxm protocol handler key
    let (nxm_key, _) = hkcu
        .create_subkey(r"Software\Classes\nxm")
        .context("failed to create nxm registry key")?;
    nxm_key.set_value("", &"URL:NXM Protocol")?;
    nxm_key.set_value("URL Protocol", &"")?;

    // Set the command that handles nxm:// URIs
    let (cmd_key, _) = hkcu
        .create_subkey(r"Software\Classes\nxm\shell\open\command")
        .context("failed to create nxm command registry key")?;
    cmd_key.set_value("", &format!("\"{}\" nxm handle \"%1\"", exe_path.display()))?;

    println!("Registered nxm:// protocol handler in Windows registry.");
    println!("You can now click 'Download with Mod Manager' on Nexus Mods.");

    Ok(exe_path)
}

#[cfg(not(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "macos", feature = "macos-integrations"),
    all(target_os = "windows", feature = "windows-integrations"),
)))]
fn install_handler_platform() -> Result<PathBuf> {
    anyhow::bail!("nxm:// protocol handler installation is not enabled for this platform build");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nxm_uri() {
        let uri = "nxm://skyrimspecialedition/mods/12604/files/35834?key=abc123&expires=1700000000";
        let parsed = NxmUri::parse(uri).unwrap();
        assert_eq!(parsed.game_domain, "skyrimspecialedition");
        assert_eq!(parsed.mod_id, NexusModId::from(12604));
        assert_eq!(parsed.file_id, NexusFileId::from(35834));
        assert_eq!(parsed.key.as_deref(), Some("abc123"));
        assert_eq!(parsed.expires, Some(1700000000));
    }

    #[test]
    fn test_parse_nxm_uri_no_query() {
        let uri = "nxm://fallout4/mods/100/files/200";
        let parsed = NxmUri::parse(uri).unwrap();
        assert_eq!(parsed.game_domain, "fallout4");
        assert_eq!(parsed.mod_id, NexusModId::from(100));
        assert_eq!(parsed.file_id, NexusFileId::from(200));
        assert!(parsed.key.is_none());
        assert!(parsed.expires.is_none());
    }

    #[test]
    fn test_parse_invalid_uri() {
        assert!(NxmUri::parse("https://example.com").is_err());
        assert!(NxmUri::parse("nxm://invalid").is_err());
        assert!(NxmUri::parse("nxm://game/bad/format").is_err());
        assert!(NxmUri::parse("nxm:///mods/100/files/200").is_err());
        assert!(NxmUri::parse("nxm://game/mods/100/files/200/extra").is_err());
        assert!(NxmUri::parse("nxm://game/mods//files/200").is_err());
        assert!(NxmUri::parse("nxm://game/mods/100/files/").is_err());
        // The domain becomes part of store, staging and dossier paths.
        assert!(NxmUri::parse("nxm://../mods/100/files/200").is_err());
        assert!(NxmUri::parse("nxm://..\\..\\x/mods/100/files/200").is_err());
    }
}
