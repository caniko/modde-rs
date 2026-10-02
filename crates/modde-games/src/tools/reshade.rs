//! `ReShade` — shader injection for Wine/Proton games.
//!
//! `ReShade` works by placing a proxy DLL (typically `dxgi.dll` or `d3d11.dll`)
//! in the game directory. Wine needs `WINEDLLOVERRIDES` set to load the native
//! version instead of its built-in stub.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use smallvec::{SmallVec, smallvec};
use tracing::info;

use super::{
    AppliedFiles, GameTool, ToolApplyPreview, ToolAvailability, ToolCategory, ToolConfig,
    ToolGameContext,
};

pub static RESHADE: ReShade = ReShade;

pub struct ReShade;

impl GameTool for ReShade {
    fn tool_id(&self) -> &'static str {
        "reshade"
    }

    fn display_name(&self) -> &'static str {
        "ReShade"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::PostProcess
    }

    fn description(&self) -> &'static str {
        "Wine/Proton ReShade deployment through proxy DLLs and shader directories."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        vec![
            super::ToolSettingSpec::path(
                "source_dir",
                "Source directory",
                "Directory containing ReShade DLLs, ReShade.ini, and shader folders.",
            )
            .section("Source"),
            super::ToolSettingSpec::select(
                "dll_name",
                "Proxy DLL",
                "DLL name copied into the executable directory.",
                &["dxgi.dll", "d3d11.dll", "dinput8.dll"],
            )
            .section("Deployment"),
            super::ToolSettingSpec::read_only(
                "derived_executable_dir",
                "Executable directory",
                "Derived from the selected game's metadata.",
            )
            .section("Detected Game"),
        ]
    }

    fn detect_available(&self) -> ToolAvailability {
        // ReShade DLLs must be provided by the user (they're Windows binaries).
        // Check if a source path is configured.
        ToolAvailability::Available {
            version: Some("user-provided".into()),
        }
    }

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::new()
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        let dll_name = config.get_str("dll_name").unwrap_or("dxgi");
        smallvec![dll_name.to_string()]
    }

    fn apply(&self, game_dir: &Path, config: &ToolConfig) -> Result<AppliedFiles> {
        self.apply_for(game_dir, None, config)
    }

    fn apply_for(
        &self,
        game_dir: &Path,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<AppliedFiles> {
        let source_dir = config
            .get_str("source_dir")
            .map(PathBuf::from)
            .context("reshade: 'source_dir' setting is required (path to ReShade DLLs)")?;

        let dll_name = config.get_str("dll_name").unwrap_or("dxgi.dll");
        let target_dir = context
            .and_then(|context| context.executable_dir.clone())
            .unwrap_or_else(|| {
                let exe_subdir = config.get_str("exe_subdir").unwrap_or("");
                if exe_subdir.is_empty() {
                    game_dir.to_path_buf()
                } else {
                    game_dir.join(exe_subdir)
                }
            });

        std::fs::create_dir_all(&target_dir)
            .with_context(|| format!("failed to create {}", target_dir.display()))?;

        let mut applied = AppliedFiles::default();

        // Copy the main DLL
        let src_dll = source_dir.join(dll_name);
        if src_dll.exists() {
            let dest = target_dir.join(dll_name);
            std::fs::copy(&src_dll, &dest)
                .with_context(|| format!("failed to copy ReShade DLL to {}", dest.display()))?;
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
            info!(dll = %dll_name, "applied ReShade DLL");
        }

        // Copy ReShade.ini if present
        let ini = source_dir.join("ReShade.ini");
        if ini.exists() {
            let dest = target_dir.join("ReShade.ini");
            std::fs::copy(&ini, &dest)
                .with_context(|| format!("failed to copy ReShade.ini to {}", dest.display()))?;
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
        }

        // Copy shader directories if present
        for subdir in &["reshade-shaders", "reshade-presets"] {
            let src = source_dir.join(subdir);
            if src.is_dir() {
                let dest = target_dir.join(subdir);
                copy_dir_recursive(&src, &dest, game_dir, &mut applied)?;
            }
        }

        Ok(applied)
    }

    fn preview_apply_for(
        &self,
        game_dir: &Path,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<ToolApplyPreview> {
        let Some(source_dir) = config
            .get_str("source_dir")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
        else {
            return Ok(missing_preview(
                "reshade: source directory is not configured",
            ));
        };
        if !source_dir.is_dir() {
            return Ok(missing_preview(format!(
                "reshade: source directory does not exist: {}",
                source_dir.display()
            )));
        }

        let dll_name = config.get_str("dll_name").unwrap_or("dxgi.dll");
        let target_dir = reshade_target_dir(game_dir, context, config);
        let mut preview = ToolApplyPreview::default();

        let src_dll = source_dir.join(dll_name);
        if src_dll.is_file() {
            preview_source_file(game_dir, &src_dll, &target_dir.join(dll_name), &mut preview)?;
        } else {
            preview.missing_inputs.push(format!(
                "reshade: source DLL not found: {}",
                src_dll.display()
            ));
        }

        let ini = source_dir.join("ReShade.ini");
        if ini.is_file() {
            preview_source_file(
                game_dir,
                &ini,
                &target_dir.join("ReShade.ini"),
                &mut preview,
            )?;
        }

        for subdir in &["reshade-shaders", "reshade-presets"] {
            let src = source_dir.join(subdir);
            if src.is_dir() {
                preview_dir_recursive(game_dir, &src, &target_dir.join(subdir), &mut preview)?;
            }
        }

        Ok(preview)
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("reshade");
        config.set("dll_name", serde_json::json!("dxgi.dll"));
        config
    }
}

fn reshade_target_dir(
    game_dir: &Path,
    context: Option<&ToolGameContext>,
    config: &ToolConfig,
) -> PathBuf {
    context
        .and_then(|context| context.executable_dir.clone())
        .unwrap_or_else(|| {
            let exe_subdir = config.get_str("exe_subdir").unwrap_or("");
            if exe_subdir.is_empty() {
                game_dir.to_path_buf()
            } else {
                game_dir.join(exe_subdir)
            }
        })
}

fn missing_preview(message: impl Into<String>) -> ToolApplyPreview {
    ToolApplyPreview {
        missing_inputs: vec![message.into()],
        ..ToolApplyPreview::default()
    }
}

fn preview_source_file(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    let expected =
        std::fs::read(src).with_context(|| format!("failed to read {}", src.display()))?;
    preview_bytes(game_dir, dest, &expected, preview);
    Ok(())
}

fn preview_bytes(game_dir: &Path, dest: &Path, expected: &[u8], preview: &mut ToolApplyPreview) {
    let changed = std::fs::read(dest).map_or(true, |current| current != expected);
    let rel = dest.strip_prefix(game_dir).unwrap_or(dest).to_path_buf();
    preview.record_file(rel, changed);
}

fn preview_dir_recursive(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?
        .flatten()
    {
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if ty.is_dir() {
            preview_dir_recursive(game_dir, &src_path, &dest_path, preview)?;
        } else {
            preview_source_file(game_dir, &src_path, &dest_path, preview)?;
        }
    }
    Ok(())
}

/// Recursively copy a directory, recording all files in [`AppliedFiles`].
fn copy_dir_recursive(
    src: &Path,
    dest: &Path,
    game_dir: &Path,
    applied: &mut AppliedFiles,
) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("failed to create {}", dest.display()))?;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?
        .flatten()
    {
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dest_path, game_dir, applied)?;
        } else {
            std::fs::copy(&src_path, &dest_path)
                .with_context(|| format!("failed to copy {}", dest_path.display()))?;
            let rel = dest_path
                .strip_prefix(game_dir)
                .unwrap_or(&dest_path)
                .to_path_buf();
            applied.files.push(rel);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_reports_changed_when_destination_is_missing() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("dxgi.dll"), b"reshade").expect("source dll");
        let mut config = ReShade.default_config();
        config.set(
            "source_dir",
            serde_json::json!(source.path().display().to_string()),
        );

        let preview = ReShade
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        assert_eq!(preview.changed_files, vec![PathBuf::from("dxgi.dll")]);
        assert_eq!(preview.unchanged_files, [] as [std::path::PathBuf; 0]);
        assert_eq!(preview.missing_inputs, [] as [std::string::String; 0]);
    }

    #[test]
    fn preview_reports_unchanged_when_destination_matches() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("dxgi.dll"), b"reshade").expect("source dll");
        std::fs::write(game.path().join("dxgi.dll"), b"reshade").expect("dest dll");
        let mut config = ReShade.default_config();
        config.set(
            "source_dir",
            serde_json::json!(source.path().display().to_string()),
        );

        let preview = ReShade
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        assert_eq!(preview.changed_files, [] as [std::path::PathBuf; 0]);
        assert_eq!(preview.unchanged_files, vec![PathBuf::from("dxgi.dll")]);
        assert_eq!(preview.missing_inputs, [] as [std::string::String; 0]);
    }
}
