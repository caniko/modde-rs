//! Project-owned visual capture and rubric producer.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use modde_core::hash::sha256_hex;
use modde_ui::screenshot::{ShotOptions, all_screens, capture_to_png};

/// Arguments for the project-owned visual producer.
#[derive(Debug)]
pub struct VisualArgs {
    /// Capture output directory.
    pub output: PathBuf,
    /// Versioned capture manifest path.
    pub manifest: PathBuf,
    /// Producer report path.
    pub report: PathBuf,
    /// Configured visual-rubric executable.
    pub rubric_bin: PathBuf,
    /// Rubric preset applied to every captured screen.
    pub preset: String,
    /// Logical window width in points.
    pub width: f32,
    /// Logical window height in points.
    pub height: f32,
    /// `HiDPI` scale factor.
    pub scale: f32,
    /// modde theme name.
    pub theme: String,
}

#[derive(Debug, Deserialize)]
struct RubricVerdict {
    verdict: String,
    reason: String,
    #[serde(default)]
    anomalies: Vec<String>,
}

#[derive(Debug)]
enum Review {
    Pass {
        reason: String,
        anomalies: Vec<String>,
    },
    Fail {
        reason: String,
        anomalies: Vec<String>,
    },
    Error(String),
}

#[derive(Debug)]
struct CaptureSpec {
    id: String,
    image: PathBuf,
    image_digest: Value,
}

/// Capture every deterministic screen, write the shared manifest, and run the
/// configured rubric against every resulting PNG.
pub fn run(args: VisualArgs) -> Result<()> {
    validate_options(&args)?;
    let manifest_path = absolute_path(&args.manifest)?;
    let report_path = absolute_path(&args.report)?;
    let root = manifest_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("/"))
        .to_path_buf();
    fs::create_dir_all(&args.output)
        .with_context(|| format!("create capture output {}", args.output.display()))?;
    fs::create_dir_all(&root).with_context(|| format!("create visual root {}", root.display()))?;

    let (revision, dirty) = git_state()?;
    let options = ShotOptions {
        width: args.width,
        height: args.height,
        scale: args.scale,
        theme: args.theme.clone(),
    };
    let output = absolute_path(&args.output)?;
    let mut specs = Vec::with_capacity(all_screens().len());
    for screen in all_screens() {
        let image = output.join(format!("{}.png", screen.as_str()));
        capture_to_png(*screen, &options, &image)
            .with_context(|| format!("capture {}", screen.as_str()))?;
        let metadata = output.join(format!("{}.capture.json", screen.as_str()));
        write_json(
            &metadata,
            &json!({
                "screen": screen.as_str(),
                "state": screen.as_str(),
                "viewport": {
                    "width": args.width,
                    "height": args.height,
                    "dpr": args.scale,
                },
                "theme": args.theme,
                "locale": "en-US",
                "renderer": "iced-tiny-skia",
                "capture_backend": "modde-ui::screenshot",
            }),
        )?;
        let id = format!("{}/desktop/{}/en-US", screen.as_str(), args.theme);
        specs.push(CaptureSpec {
            id,
            image_digest: artifact_digest(&image, &root)?,
            image,
        });
    }

    let coverage_root = root.join("coverage");
    fs::create_dir_all(&coverage_root)
        .with_context(|| format!("create coverage root {}", coverage_root.display()))?;
    let contract_path = coverage_root.join("contract.json");
    let contract = coverage_contract(&revision, &specs);
    write_json(&contract_path, &contract)?;
    let contract_digest = artifact_digest(&contract_path, &root)?;

    let coverage_report_path = coverage_root.join("report.json");
    let coverage_report = coverage_report(&contract, &specs)?;
    write_json(&coverage_report_path, &coverage_report)?;
    let coverage_report_digest = artifact_digest(&coverage_report_path, &root)?;

    let captures = specs
        .iter()
        .map(|spec| {
            let stem = spec.image.file_stem().ok_or_else(|| {
                anyhow!("capture image {} has no file stem", spec.image.display())
            })?;
            let metadata_path = spec
                .image
                .with_file_name(format!("{}.capture.json", stem.to_string_lossy()));
            Ok(json!({
                "id": spec.id,
                "image": spec.image_digest,
                "state": spec.id.split('/').next().unwrap_or("screen"),
                "viewport": {
                    "width": args.width as u32,
                    "height": args.height as u32,
                    "dpr": args.scale,
                },
                "theme": args.theme,
                "locale": "en-US",
                "metadata": {
                    "capture": artifact_digest(&metadata_path, &root)?,
                },
                "presets": [args.preset],
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = json!({
        "schema_version": 3,
        "target": "modde-rs/application",
        "revision": revision,
        "dirty": dirty,
        "environment": {
            "platform": std::env::consts::OS,
            "renderer": "iced-tiny-skia",
            "capture_backend": "modde-ui::screenshot",
        },
        "declared_cells": captures.len(),
        "captures": captures,
        "coverage_contract": contract_digest,
        "coverage_report": coverage_report_digest,
    });
    write_json(&manifest_path, &manifest)?;

    let mut cells = Vec::with_capacity(specs.len());
    let mut failures = Vec::new();
    let mut errors = Vec::new();
    let mut passed_cells = 0_u64;
    for spec in &specs {
        let review = review_image(&args, &spec.image);
        let (status, details) = match review {
            Review::Pass { reason, anomalies } => {
                passed_cells += 1;
                ("pass", json!({"reason": reason, "anomalies": anomalies}))
            }
            Review::Fail { reason, anomalies } => {
                failures.push(json!({
                    "id": spec.id,
                    "reason": reason,
                    "anomalies": anomalies,
                }));
                ("fail", json!({"reason": reason, "anomalies": anomalies}))
            }
            Review::Error(message) => {
                errors.push(json!({"id": spec.id, "message": message}));
                ("error", Value::Null)
            }
        };
        cells.push(json!({
            "id": spec.id,
            "image": spec.image_digest,
            "status": status,
            "rubric": details,
        }));
    }

    let report = json!({
        "schema_version": 3,
        "target": "modde-rs/application",
        "git": {"sha": revision, "dirty": dirty},
        "capture_manifest": manifest_path.strip_prefix(&root).unwrap_or(&manifest_path),
        "cells": cells,
        "summary": {
            "total_cells": specs.len(),
            "passed_cells": passed_cells,
            "failed_cells": failures.len(),
            "error_cells": errors.len(),
        },
        "failures": failures,
        "errors": errors,
        "rate_limit_events": [],
    });
    write_json(&report_path, &report)?;

    if !failures.is_empty() {
        bail!("visual rubric found {} failing cell(s)", failures.len());
    }
    if !errors.is_empty() {
        bail!("visual rubric could not evaluate {} cell(s)", errors.len());
    }
    Ok(())
}

fn validate_options(args: &VisualArgs) -> Result<()> {
    for (name, value) in [
        ("width", args.width),
        ("height", args.height),
        ("scale", args.scale),
    ] {
        if !value.is_finite() || value <= 0.0 {
            bail!("{name} must be finite and greater than zero");
        }
    }
    if args.preset.trim().is_empty() {
        bail!("preset must not be empty");
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn git_state() -> Result<(String, bool)> {
    let revision = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .context("read git revision")?
            .stdout,
    )?
    .trim()
    .to_owned();
    if revision.is_empty() {
        bail!("git revision is empty");
    }
    let status = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .context("read git dirty state")?;
    if !status.status.success() {
        bail!("git status failed");
    }
    Ok((revision, !status.stdout.is_empty()))
}

fn artifact_digest(path: &Path, root: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("read artifact {}", path.display()))?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| anyhow!("artifact {} is outside {}", path.display(), root.display()))?;
    Ok(json!({
        "path": relative,
        "sha256": sha256_hex(&bytes),
        "bytes": bytes.len(),
    }))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn coverage_contract(revision: &str, specs: &[CaptureSpec]) -> Value {
    let surfaces = specs
        .iter()
        .map(|spec| {
            json!({
                "id": spec.id,
                "platform": "linux-offscreen",
                "required": true,
                "description": format!("{} populated demo screen", spec.id.split('/').next().unwrap_or("modde")),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": 1,
        "target": "modde-rs/application",
        "revision": revision,
        "surfaces": surfaces,
        "transitions": [{
            "id": "interactive/desktop",
            "from": "",
            "action": "interactive_event_loop",
            "to": "",
            "platform": "linux-offscreen",
            "required": false,
        }],
        "exclusions": [{
            "id": "interactive/desktop",
            "reason": "The producer renders static in-memory demo states and does not drive the live event loop.",
            "owner": "modde-rs visual producer",
            "review_after": "2026-10-01",
        }],
    })
}

fn coverage_report(contract: &Value, specs: &[CaptureSpec]) -> Result<Value> {
    let contract_sha256 = sha256_hex(&serde_json::to_vec(contract)?);
    let ids = specs.iter().map(|spec| spec.id.clone()).collect::<Vec<_>>();
    Ok(json!({
        "schema_version": 1,
        "contract_sha256": contract_sha256,
        "planned_ids": ids,
        "executed_ids": specs.iter().map(|spec| spec.id.clone()).collect::<Vec<_>>(),
        "passed_ids": specs.iter().map(|spec| spec.id.clone()).collect::<Vec<_>>(),
        "failed_ids": [],
        "blocked_ids": [],
    }))
}

fn review_image(args: &VisualArgs, image: &Path) -> Review {
    let output = match Command::new(&args.rubric_bin)
        .arg("configured")
        .arg("--image")
        .arg(image)
        .arg("--preset")
        .arg(&args.preset)
        .arg("--json")
        .output()
    {
        Ok(output) => output,
        Err(error) => return Review::Error(format!("spawn visual-rubric: {error}")),
    };
    if !output.status.success() {
        return Review::Error(format!(
            "visual-rubric exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let verdict = match serde_json::from_slice::<RubricVerdict>(&output.stdout) {
        Ok(verdict) => verdict,
        Err(error) => return Review::Error(format!("parse visual-rubric JSON: {error}")),
    };
    match verdict.verdict.as_str() {
        "pass" => Review::Pass {
            reason: verdict.reason,
            anomalies: verdict.anomalies,
        },
        "fail" => Review::Fail {
            reason: verdict.reason,
            anomalies: verdict.anomalies,
        },
        other => Review::Error(format!("visual-rubric returned unknown verdict {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_report_accounts_for_every_capture_surface() {
        let specs = vec![CaptureSpec {
            id: "mod-list/desktop/Dark/en-US".to_owned(),
            image: PathBuf::from("captures/mod-list.png"),
            image_digest: json!({"path": "captures/mod-list.png", "sha256": "a".repeat(64), "bytes": 1}),
        }];
        let contract = coverage_contract("deadbeef", &specs);
        let report = coverage_report(&contract, &specs).expect("coverage report");
        assert_eq!(report["planned_ids"], json!([specs[0].id]));
        assert_eq!(report["passed_ids"], report["executed_ids"]);
    }
}
