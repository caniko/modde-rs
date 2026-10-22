//! `extract_optiscaler_archive_flat` flattens a release archive into the
//! layout `OptiScaler` expects, keeping FSR4 variants and plugins apart.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use modde_games::tools::optiscaler::extract_optiscaler_archive_flat;

const RELEASE: &[&str] = &[
    "OptiScaler_0.9/OptiScaler.dll",
    "OptiScaler_0.9/OptiScaler.ini",
    "OptiScaler_0.9/FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll",
    "OptiScaler_0.9/FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll",
    "OptiScaler_0.9/plugins/OptiPatcher.asi",
    "OptiScaler_0.9/readme.txt",
];

const FLATTENED: &[&str] = &[
    "FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll",
    "FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll",
    "OptiScaler.dll",
    "OptiScaler.ini",
    "plugins/OptiPatcher.asi",
];

/// The 7z path extracts under `modde_data_dir()/tmp`; keep it sandboxed.
fn isolated_data_dir() {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        modde_core::paths::set_data_dir(dir.path().to_path_buf());
        dir
    });
}

fn zip_archive(path: &Path, entries: &[&str]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for name in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(name.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn files_under(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn zip_releases_are_flattened_by_payload_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("OptiScaler.ZIP");
    zip_archive(&archive, RELEASE);
    let dest = tmp.path().join("out");

    extract_optiscaler_archive_flat(&archive, &dest).unwrap();
    assert_eq!(files_under(&dest), FLATTENED);
    assert_eq!(
        std::fs::read_to_string(dest.join("FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll")).unwrap(),
        "OptiScaler_0.9/FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll"
    );
}

#[test]
fn archives_without_optiscaler_or_of_unknown_type_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("other.zip");
    zip_archive(&archive, &["dxgi.dll", "readme.txt"]);
    let err = extract_optiscaler_archive_flat(&archive, &tmp.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("did not contain OptiScaler.dll"),
        "{err}"
    );

    let rar = tmp.path().join("OptiScaler.rar");
    std::fs::write(&rar, b"rar").unwrap();
    let err = extract_optiscaler_archive_flat(&rar, &tmp.path().join("out")).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported OptiScaler archive type"),
        "{err}"
    );
}

fn seven_zip() -> Option<&'static str> {
    ["7zz", "7z"].into_iter().find(|bin| {
        std::process::Command::new(bin)
            .arg("i")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

#[test]
fn seven_zip_releases_are_flattened_too() {
    let Some(bin) = seven_zip() else {
        eprintln!("skipping: neither 7zz nor 7z is on PATH");
        return;
    };
    isolated_data_dir();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    for name in RELEASE {
        let path = src.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, name).unwrap();
    }
    let archive: PathBuf = tmp.path().join("OptiScaler.7z");
    let status = std::process::Command::new(bin)
        .current_dir(&src)
        .args(["a", "-y", archive.to_str().unwrap(), "OptiScaler_0.9"])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());

    let dest = tmp.path().join("out");
    extract_optiscaler_archive_flat(&archive, &dest).unwrap();
    assert_eq!(files_under(&dest), FLATTENED);
    // The scratch extraction directory is cleaned up afterwards.
    let scratch = modde_core::paths::modde_data_dir().join("tmp");
    assert!(std::fs::read_dir(scratch).unwrap().next().is_none());

    let not_7z = tmp.path().join("broken.7z");
    std::fs::write(&not_7z, b"not 7z").unwrap();
    assert!(extract_optiscaler_archive_flat(&not_7z, &dest).is_err());
}
