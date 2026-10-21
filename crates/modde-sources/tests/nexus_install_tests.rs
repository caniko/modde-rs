//! `install_single_mod` end to end against a mocked Nexus API and CDN.

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

use modde_core::installer::{InstallMethod, InstallProbe};
use modde_core::{NexusFileId, NexusModId, paths};
use modde_sources::nexus::api::NexusMod;
use modde_sources::nexus::install::{InstallOutcome, install_single_mod};
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DOMAIN: &str = "skyrimspecialedition";

/// Serialises `MODDE_NEXUS_BASE_URL`, which every request reads.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn isolated_data_dir() {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        paths::set_data_dir(dir.path().to_path_buf());
        dir
    });
}

fn zip(entries: &[&str]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    for name in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap();
    cursor.into_inner()
}

fn mod_info() -> NexusMod {
    serde_json::from_value(json!({
        "mod_id": 1, "name": "Mod", "summary": null, "version": "1.0", "author": "a"
    }))
    .unwrap()
}

/// A Nexus API that reports `premium` and serves `archive` from the CDN
/// link (404 when `None`).
async fn nexus(premium: bool, archive: Option<Vec<u8>>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "is_premium": premium })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/games/.+/download_link\.json$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{ "URI": format!("{}/cdn/mod.zip", server.uri()) }])),
        )
        .mount(&server)
        .await;
    let cdn = match archive {
        Some(bytes) => ResponseTemplate::new(200).set_body_bytes(bytes),
        None => ResponseTemplate::new(404),
    };
    Mock::given(method("GET"))
        .and(path("/cdn/mod.zip"))
        .respond_with(cdn)
        .mount(&server)
        .await;
    server
}

/// Run the pipeline against `server` for a mod id unique to the test, so
/// tests sharing the process-wide data dir never collide.
async fn install(
    server: &MockServer,
    mod_id: u64,
    probe: &InstallProbe,
) -> anyhow::Result<InstallOutcome> {
    isolated_data_dir();
    // SAFETY: every caller holds ENV_LOCK for the whole test.
    unsafe { std::env::set_var("MODDE_NEXUS_BASE_URL", server.uri()) };
    install_single_mod(
        &reqwest::Client::new(),
        "key",
        DOMAIN,
        NexusModId::from(mod_id),
        NexusFileId::from(1),
        &mod_info(),
        probe,
    )
    .await
}

fn store_dir(mod_id: u64) -> PathBuf {
    paths::store_dir().join(format!("{DOMAIN}_{mod_id}_1"))
}

fn staging_dir(mod_id: u64) -> PathBuf {
    paths::staging_dir().join(format!("install_{DOMAIN}_{mod_id}_1"))
}

fn error_chain(result: anyhow::Result<InstallOutcome>) -> String {
    match result {
        Ok(_) => panic!("expected the install to fail"),
        Err(e) => format!("{e:#}"),
    }
}

#[tokio::test]
async fn ready_archive_installs_into_the_store_once() {
    let _env = ENV_LOCK.lock().await;
    let server = nexus(true, Some(zip(&["version.dll"]))).await;
    isolated_data_dir();
    // A leftover staging tree from an interrupted install is replaced.
    std::fs::create_dir_all(staging_dir(10).join("stale")).unwrap();

    let InstallOutcome::Installed(plan) =
        install(&server, 10, &InstallProbe::noop()).await.unwrap()
    else {
        panic!("expected Installed");
    };
    assert!(matches!(plan.method, InstallMethod::DllOverlay { .. }));
    assert_eq!(plan.staged_files.len(), 1);
    assert!(store_dir(10).join("__dll_overlay__/version.dll").is_file());
    assert!(!staging_dir(10).exists());
    assert!(!paths::store_dir().join("10_1.zip").exists());

    assert!(matches!(
        install(&server, 10, &InstallProbe::noop()).await.unwrap(),
        InstallOutcome::AlreadyStaged
    ));
}

#[tokio::test]
async fn unconfigured_installers_are_staged_for_user_input() {
    let _env = ENV_LOCK.lock().await;
    let fomod = nexus(true, Some(zip(&["fomod/ModuleConfig.xml", "Data/a.esp"]))).await;
    assert!(matches!(
        install(&fomod, 20, &InstallProbe::noop()).await.unwrap(),
        InstallOutcome::PendingUserInput { method: "fomod" }
    ));
    // The extracted tree is kept as the wizard's working copy.
    assert!(store_dir(20).join("fomod/ModuleConfig.xml").is_file());
    assert!(store_dir(20).join("Data/a.esp").is_file());

    let bain = nexus(true, Some(zip(&["00 Core/a.esp", "10 Option/b.esp"]))).await;
    assert!(matches!(
        install(&bain, 21, &InstallProbe::noop()).await.unwrap(),
        InstallOutcome::PendingUserInput { method: "bain" }
    ));

    let merge = InstallProbe {
        analyze: Box::new(|_| {
            Some(InstallMethod::ScriptMerge {
                merge_group: "scripts".into(),
                base: Box::new(InstallMethod::Bain {
                    selected_subdirs: Vec::new(),
                }),
            })
        }),
        ..InstallProbe::noop()
    };
    let other = nexus(true, Some(zip(&["a.psc"]))).await;
    assert!(matches!(
        install(&other, 22, &merge).await.unwrap(),
        InstallOutcome::PendingUserInput { method: "unknown" }
    ));
}

#[tokio::test]
async fn unknown_layout_writes_a_dossier() {
    let _env = ENV_LOCK.lock().await;
    let server = nexus(true, Some(zip(&["readme.txt"]))).await;

    let InstallOutcome::Unknown {
        dossier_path,
        method,
    } = install(&server, 30, &InstallProbe::noop()).await.unwrap()
    else {
        panic!("expected Unknown");
    };
    assert!(matches!(method, InstallMethod::Unknown { .. }));
    assert_eq!(
        dossier_path,
        modde_core::installer::dossiers_dir().join(format!("{DOMAIN}_30_1"))
    );
    assert!(dossier_path.join("metadata.json").is_file());
    assert!(!store_dir(30).exists());
    assert!(!staging_dir(30).exists());
}

#[tokio::test]
async fn execution_errors_propagate() {
    let _env = ENV_LOCK.lock().await;
    let server = nexus(true, Some(zip(&["fomod/ModuleConfig.xml"]))).await;
    let broken_config = InstallProbe {
        analyze: Box::new(|_| {
            Some(InstallMethod::Fomod {
                module_config: "fomod/ModuleConfig.xml".into(),
                config_toml: Some("not = [toml".into()),
            })
        }),
        ..InstallProbe::noop()
    };

    assert!(
        error_chain(install(&server, 40, &broken_config).await).contains("invalid config TOML")
    );
}

#[tokio::test]
async fn failures_before_extraction_are_reported() {
    let _env = ENV_LOCK.lock().await;
    let probe = InstallProbe::noop();

    let free = nexus(false, Some(zip(&["version.dll"]))).await;
    assert!(error_chain(install(&free, 50, &probe).await).contains("Nexus Premium is required"));

    let missing = nexus(true, None).await;
    assert!(
        error_chain(install(&missing, 51, &probe).await).contains("failed to download mod archive")
    );

    let corrupt = nexus(true, Some(b"not a zip".to_vec())).await;
    assert!(
        error_chain(install(&corrupt, 52, &probe).await).contains("failed to extract mod archive")
    );

    isolated_data_dir();
    let rejected = install_single_mod(
        &reqwest::Client::new(),
        "key",
        "../escape",
        NexusModId::from(53),
        NexusFileId::from(1),
        &mod_info(),
        &probe,
    )
    .await;
    assert!(error_chain(rejected).contains("invalid Nexus game domain"));
}
