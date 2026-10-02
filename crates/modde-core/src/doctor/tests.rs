use super::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn profile_snapshot_diff_detects_common_changes() {
    let previous = ProfileSnapshotRow {
        id: 7,
        created_at: "2026-06-11 10:00:00".into(),
        snapshot: vec![
            DoctorProfileModSnapshot {
                mod_id: "a".into(),
                display_name: None,
                version: Some("1".into()),
                source_archive_hash: None,
                enabled: true,
            },
            DoctorProfileModSnapshot {
                mod_id: "b".into(),
                display_name: None,
                version: Some("1".into()),
                source_archive_hash: None,
                enabled: true,
            },
        ],
    };
    let current = vec![
        DoctorProfileModSnapshot {
            mod_id: "a".into(),
            display_name: None,
            version: Some("2".into()),
            source_archive_hash: None,
            enabled: false,
        },
        DoctorProfileModSnapshot {
            mod_id: "c".into(),
            display_name: None,
            version: None,
            source_archive_hash: None,
            enabled: true,
        },
    ];

    let kinds = diff_profile_snapshots(&current, &previous)
        .into_iter()
        .map(|diff| diff.kind)
        .collect::<Vec<_>>();
    assert!(kinds.contains(&DoctorProfileDiffKind::VersionChanged));
    assert!(kinds.contains(&DoctorProfileDiffKind::Disabled));
    assert!(kinds.contains(&DoctorProfileDiffKind::Removed));
    assert!(kinds.contains(&DoctorProfileDiffKind::Added));
}

#[test]
fn citation_validation_rejects_unsupported_hypotheses() {
    let evidence = vec![DoctorEvidence {
        id: "MOD-0001".into(),
        kind: "mod".into(),
        title: "A".into(),
        detail: "detail".into(),
    }];
    let explanation = DoctorExplanation {
        hypotheses: vec![
            DoctorHypothesis {
                rank: 1,
                title: "ok".into(),
                confidence: "high".into(),
                summary: "summary".into(),
                evidence_ids: vec!["MOD-0001".into()],
                recommended_checks: vec![],
            },
            DoctorHypothesis {
                rank: 2,
                title: "bad".into(),
                confidence: "high".into(),
                summary: "summary".into(),
                evidence_ids: vec!["NOPE".into()],
                recommended_checks: vec![],
            },
        ],
        unsupported: vec![],
    };
    let validated = validate_explanation(explanation, &evidence);
    assert_eq!(validated.hypotheses.len(), 1);
    assert_eq!(validated.unsupported.len(), 1);
}

#[tokio::test]
async fn openai_compatible_client_supports_local_and_remote() {
    let server = MockServer::start().await;
    let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "choices": [{
            "message": {
                "content": "{\"hypotheses\":[{\"rank\":1,\"title\":\"Grounded\",\"confidence\":\"high\",\"summary\":\"Uses provided evidence\",\"evidence_ids\":[\"MOD-0001\"],\"recommended_checks\":[\"Disable test mod\"]}],\"unsupported\":[]}"
            }
        }]
    }));
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(response.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/remote/chat/completions"))
        .and(header("authorization", "Bearer secret"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;

    let context = DoctorContext {
        game_id: "skyrim-se".into(),
        profile_name: "test".into(),
        crash: None,
        mod_set: vec![],
        active_plugins: vec![],
        installed_files: vec![],
        tool_files: vec![],
        diagnostics: vec![],
        collisions: vec![],
        recent_profile_diffs: vec![],
        evidence: vec![DoctorEvidence {
            id: "MOD-0001".into(),
            kind: "mod".into(),
            title: "Test Mod".into(),
            detail: "enabled".into(),
        }],
    };
    let settings = DoctorLlmSettings {
        local_endpoint: format!("{}/v1", server.uri()),
        local_model: "local".into(),
        remote_endpoint: Some(format!("{}/remote", server.uri())),
        remote_model: Some("remote".into()),
        remote_api_key_env: "MODDE_TEST_DOCTOR_REMOTE_KEY".into(),
        timeout_seconds: 5,
        max_context_bytes: 64 * 1024,
    };

    let local = explain_with_openai_compatible_chat(
        &settings,
        DoctorLlmRequest {
            provider: DoctorLlmProvider::Local,
            context: context.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(local.hypotheses.len(), 1);

    // SAFETY: this test uses a unique env var name and no other test in
    // this crate reads it.
    unsafe {
        std::env::set_var("MODDE_TEST_DOCTOR_REMOTE_KEY", "secret");
    }
    let remote = explain_with_openai_compatible_chat(
        &settings,
        DoctorLlmRequest {
            provider: DoctorLlmProvider::Remote,
            context,
        },
    )
    .await
    .unwrap();
    assert_eq!(remote.hypotheses.len(), 1);
    // SAFETY: this cleans up the unique env var set above.
    unsafe {
        std::env::remove_var("MODDE_TEST_DOCTOR_REMOTE_KEY");
    }
}

// ── cap_evidence_string / trim_doctor_context ──────────────────

use std::path::PathBuf;

use crate::crash::{CrashConfidence, CrashLogFormat, CrashSignature, CrashSuspect};

fn sample_crash() -> CrashCorrelationReport {
    CrashCorrelationReport {
        game_id: "skyrim-se".into(),
        profile_name: "test".into(),
        source_path: PathBuf::from("/tmp/crash.log"),
        raw_sha256: "abc123".into(),
        format: CrashLogFormat::Generic,
        signature: CrashSignature {
            detected_format: CrashLogFormat::Generic,
            exception_line: None,
            tokens: vec![],
        },
        suspects: vec![CrashSuspect {
            mod_id: Some("suspect-mod".into()),
            display_name: None,
            version: None,
            nexus_mod_id: None,
            nexus_file_id: None,
            nexus_game_domain: None,
            installed_timestamp: None,
            plugin_name: Some("suspect.esp".into()),
            plugin_load_index: Some(12),
            evidence: vec![CrashEvidence {
                token: "suspect.esp".into(),
                kind: CrashTokenKind::Plugin,
                section: "call stack".into(),
                reason: "plugin named in crashing frame".into(),
                line: 12,
            }],
            confidence: CrashConfidence::High,
            summary: "suspect.esp appears in the crashing call stack".into(),
        }],
    }
}

/// A context carrying every trimmable section: tool files, installed
/// files, collisions, crash evidence, profile diffs, and diagnostics.
fn populated_context() -> DoctorContext {
    let mut ctx = DoctorContext {
        game_id: "skyrim-se".into(),
        profile_name: "test".into(),
        crash: Some(sample_crash()),
        mod_set: vec![DoctorProfileModSnapshot {
            mod_id: "mod-a".into(),
            display_name: None,
            version: Some("1".into()),
            source_archive_hash: None,
            enabled: true,
        }],
        active_plugins: vec![DoctorPluginSummary {
            plugin_name: "a.esp".into(),
            sort_index: 0,
            enabled: true,
        }],
        installed_files: (0..8)
            .map(|i| DoctorInstalledFileSummary {
                mod_id: "mod-a".into(),
                rel_path: format!("textures/file-{i}.dds"),
                origin_rel_path: format!("textures/file-{i}.dds"),
                size: 1024,
                merge_group: None,
            })
            .collect(),
        tool_files: (0..4).map(|i| format!("tool/output-{i}.txt")).collect(),
        diagnostics: vec![DoctorDiagnosticSummary {
            severity: "warning".into(),
            title: "Loose file shadowing".into(),
            detail: "a loose file shadows an archive entry".into(),
            affected_mod: Some("mod-a".into()),
            affected_file: None,
        }],
        collisions: vec![DoctorCollisionSummary {
            loser: "mod-a".into(),
            winner: "mod-b".into(),
            severity: "dangerous".into(),
            files: vec!["meshes/clutter.nif".into()],
        }],
        recent_profile_diffs: vec![DoctorProfileDiff {
            kind: DoctorProfileDiffKind::VersionChanged,
            mod_id: "mod-a".into(),
            display_name: None,
            previous_version: Some("1".into()),
            current_version: Some("2".into()),
            snapshot_id: 1,
            snapshot_created_at: "2026-06-11 10:00:00".into(),
        }],
        evidence: Vec::new(),
    };
    ctx.evidence = build_evidence(&ctx);
    ctx
}

fn context_bytes(ctx: &DoctorContext) -> usize {
    serde_json::to_string_pretty(ctx).unwrap().len()
}

#[test]
fn cap_evidence_string_keeps_exact_limit_and_truncates_above_it() {
    let exact = "x".repeat(MAX_EVIDENCE_STRING_CHARS);
    assert_eq!(cap_evidence_string(&exact), exact);

    // One char over: output is the first 100 chars plus a "..." marker.
    let over = format!("{exact}y");
    let capped = cap_evidence_string(&over);
    assert_eq!(capped, format!("{exact}..."));
    assert_eq!(capped.chars().count(), MAX_EVIDENCE_STRING_CHARS + 3);
}

#[test]
fn trim_doctor_context_keeps_everything_when_within_limit() {
    let ctx = populated_context();
    let limit = context_bytes(&ctx);

    let trimmed = trim_doctor_context(ctx.clone(), limit).unwrap();
    assert_eq!(trimmed.tool_files.len(), ctx.tool_files.len());
    assert_eq!(trimmed.installed_files.len(), ctx.installed_files.len());
    assert_eq!(trimmed.collisions.len(), ctx.collisions.len());
}

#[test]
fn trim_doctor_context_drops_tool_files_first() {
    let ctx = populated_context();
    let mut target = ctx.clone();
    target.tool_files.clear();
    target.evidence = build_evidence(&target);
    let limit = context_bytes(&target);
    assert!(limit < context_bytes(&ctx), "limit must force trimming");

    let trimmed = trim_doctor_context(ctx.clone(), limit).unwrap();
    assert_eq!(trimmed.tool_files, [] as [std::string::String; 0]);
    assert_eq!(trimmed.installed_files.len(), ctx.installed_files.len());
    assert_eq!(trimmed.collisions.len(), ctx.collisions.len());
    assert!(trimmed.crash.is_some());
}

#[test]
fn trim_doctor_context_shrinks_installed_files_after_tool_files() {
    let ctx = populated_context();
    let mut target = ctx.clone();
    target.tool_files.clear();
    target.installed_files.truncate(2);
    target.evidence = build_evidence(&target);
    let limit = context_bytes(&target);

    let trimmed = trim_doctor_context(ctx.clone(), limit).unwrap();
    assert_eq!(trimmed.tool_files, [] as [std::string::String; 0]);
    // The loop halves 8 -> 4 -> 2 and stops once the context fits.
    assert_eq!(trimmed.installed_files.len(), 2);
    assert_eq!(trimmed.collisions.len(), ctx.collisions.len());
    assert!(trimmed.crash.is_some());
}

#[test]
fn trim_doctor_context_drops_collisions_but_keeps_crash_diffs_diagnostics() {
    let ctx = populated_context();
    let mut target = ctx.clone();
    target.tool_files.clear();
    target.installed_files.clear();
    target.collisions.clear();
    target.evidence = build_evidence(&target);
    let limit = context_bytes(&target);

    let trimmed = trim_doctor_context(ctx, limit).unwrap();
    assert_eq!(trimmed.tool_files, [] as [std::string::String; 0]);
    assert!(trimmed.installed_files.is_empty());
    assert!(trimmed.collisions.is_empty());
    assert!(trimmed.crash.is_some());
    assert_ne!(trimmed.recent_profile_diffs.len(), 0);
    assert!(!trimmed.diagnostics.is_empty());
    assert!(
        trimmed.evidence.iter().any(|e| e.kind == "crash-suspect"),
        "crash suspects must survive trimming"
    );
}

#[test]
fn trim_doctor_context_retains_only_priority_evidence_as_last_resort() {
    let ctx = populated_context();
    let mut target = ctx.clone();
    target.tool_files.clear();
    target.installed_files.clear();
    target.collisions.clear();
    target.evidence = build_evidence(&target);
    target.evidence.retain(|e| {
        matches!(
            e.kind.as_str(),
            "crash-suspect" | "profile-diff" | "diagnostic"
        )
    });
    let limit = context_bytes(&target);

    let trimmed = trim_doctor_context(ctx, limit).unwrap();
    assert!(!trimmed.evidence.is_empty());
    assert!(
        trimmed.evidence.iter().all(|e| matches!(
            e.kind.as_str(),
            "crash-suspect" | "profile-diff" | "diagnostic"
        )),
        "only crash/diff/diagnostic evidence may survive the last trim step"
    );
}

#[test]
fn trim_doctor_context_errors_when_minimal_context_exceeds_limit() {
    let error = trim_doctor_context(populated_context(), 10).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("above configured max_context_bytes"),
        "unexpected error: {error}"
    );
}
