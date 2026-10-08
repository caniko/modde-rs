use std::io::Read;
use std::path::Path;

use modde_core::manifest::wabbajack::WabbajackManifest;

#[test]
fn parse_real_wabbajack_file() {
    let Ok(wj_path) = std::env::var("MODDE_3077_WABBAJACK") else {
        eprintln!("skipping: MODDE_3077_WABBAJACK is not set");
        return;
    };
    let wj_path = Path::new(&wj_path);

    if !wj_path.exists() {
        eprintln!(
            "skipping: MODDE_3077_WABBAJACK not found at {}",
            wj_path.display()
        );
        return;
    }

    let file = std::fs::File::open(wj_path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();

    let json_str = {
        let name = if archive.by_name("modlist").is_ok() {
            "modlist"
        } else {
            "modlist.json"
        };
        let mut entry = archive.by_name(name).unwrap();
        let mut s = String::new();
        entry.read_to_string(&mut s).unwrap();
        s
    };

    let manifest: WabbajackManifest =
        serde_json::from_str(&json_str).expect("failed to parse MODDE_3077_WABBAJACK manifest");

    assert_eq!(manifest.name, "3077_v2");
    assert_eq!(manifest.author, "Ultra Place");
    assert_eq!(manifest.game, "Cyberpunk2077");
    assert_eq!(manifest.archives.len(), 453);
    assert_eq!(manifest.directives.len(), 7393);

    let downloads = manifest.download_directives();
    assert_eq!(downloads.len(), 453);

    let installs = manifest.install_directives();
    // Should be 6912 FromArchive + 477 InlineFile + 4 PatchedFromArchive = 7393
    // (CreateBSA = 0 for this modlist)
    assert_eq!(installs.len(), 7393);

    let mut from_archive = 0usize;
    let mut inline = 0usize;
    let mut patched = 0usize;
    for d in &installs {
        match d {
            modde_core::InstallDirective::FromArchive { .. } => from_archive += 1,
            modde_core::InstallDirective::InlineFile { .. } => inline += 1,
            modde_core::InstallDirective::PatchedFromArchive { .. } => patched += 1,
            modde_core::InstallDirective::CreateBSA { .. } => {}
        }
    }
    assert_eq!(from_archive, 6912);
    assert_eq!(inline, 477);
    assert!(patched >= 3); // at least 3 PatchedFromArchive directives

    // Verify first archive hash is nonzero (base64 decoded correctly)
    let first = &manifest.archives[0];
    assert_ne!(first.hash, 0, "hash should be nonzero after base64 decode");
    assert_ne!(first.name, "");
}
