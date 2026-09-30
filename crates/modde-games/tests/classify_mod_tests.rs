#![cfg(all(feature = "bethesda", feature = "cyberpunk"))]

//! Tests for `classify_mod_by_content` and the per-game `classify_mod` methods.

use modde_games::bethesda::{FALLOUT4, SKYRIM_SE};
use modde_games::cyberpunk::Cyberpunk2077;
use modde_games::{GamePlugin, ModClassifyConfig, ModSafety, classify_mod_by_content};

// ── classify_mod_by_content shared walker ────────────────────────────────────

const TEST_CONFIG: ModClassifyConfig = ModClassifyConfig {
    save_breaking_ext: &["lua", "esp"],
    cosmetic_ext: &["dds", "nif"],
    save_breaking_dirs: &["scripts/plugins"],
};

#[test]
fn classify_empty_dir_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::Unknown
    );
}

#[test]
fn classify_nonexistent_dir_is_unknown() {
    assert_eq!(
        classify_mod_by_content(
            std::path::Path::new("/tmp/this/does/not/exist/xyz12345"),
            &TEST_CONFIG
        ),
        ModSafety::Unknown
    );
}

#[test]
fn classify_cosmetic_only_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("texture.dds"), b"dds data").unwrap();
    std::fs::write(tmp.path().join("mesh.nif"), b"nif data").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::SaveSafe
    );
}

#[test]
fn classify_save_breaking_ext_wins_over_cosmetic() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("texture.dds"), b"dds data").unwrap();
    std::fs::write(tmp.path().join("quest.esp"), b"esp data").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::SaveBreaking
    );
}

#[test]
fn classify_script_ext_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mod.lua"), b"lua script").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::SaveBreaking
    );
}

#[test]
fn classify_unknown_ext_only_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("readme.txt"), b"text").unwrap();
    std::fs::write(tmp.path().join("data.bin"), b"binary").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::Unknown
    );
}

#[test]
fn classify_save_breaking_dir_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let scripts_dir = tmp.path().join("scripts/plugins");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::write(scripts_dir.join("somemod.xyz"), b"data").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::SaveBreaking
    );
}

#[test]
fn classify_nested_cosmetic_files() {
    let tmp = tempfile::tempdir().unwrap();
    let subdir = tmp.path().join("textures/actors");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("hero.dds"), b"dds").unwrap();
    assert_eq!(
        classify_mod_by_content(tmp.path(), &TEST_CONFIG),
        ModSafety::SaveSafe
    );
}

// ── Cyberpunk2077::classify_mod ───────────────────────────────────────────────

#[test]
fn cyberpunk_classify_reds_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mymod.reds"), b"redscript").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_lua_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("init.lua"), b"lua").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_tweak_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("damage.tweak"), b"tweak").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_xl_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mod.xl"), b"xl").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_yaml_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("config.yaml"), b"yaml").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_cet_dir_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    let cet = tmp
        .path()
        .join("bin/x64/plugins/cyber_engine_tweaks/mods/mymod");
    std::fs::create_dir_all(&cet).unwrap();
    std::fs::write(cet.join("init.lua"), b"lua").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_r6_scripts_dir_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    let scripts = tmp.path().join("r6/scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("mod.reds"), b"reds").unwrap();
    assert_eq!(
        Cyberpunk2077.classify_mod(tmp.path()),
        ModSafety::SaveBreaking
    );
}

#[test]
fn cyberpunk_classify_archive_only_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mesh_replacer.archive"), b"archive").unwrap();
    assert_eq!(Cyberpunk2077.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn cyberpunk_classify_texture_only_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("skin.dds"), b"dds").unwrap();
    assert_eq!(Cyberpunk2077.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn cyberpunk_classify_empty_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(Cyberpunk2077.classify_mod(tmp.path()), ModSafety::Unknown);
}

// ── Bethesda (SKYRIM_SE) classify_mod ────────────────────────────────────────

#[test]
fn bethesda_classify_esp_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("MyMod.esp"), b"TES4").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveBreaking);
}

#[test]
fn bethesda_classify_esm_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("DLC.esm"), b"TES4").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveBreaking);
}

#[test]
fn bethesda_classify_esl_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("light_plugin.esl"), b"TES4").unwrap();
    assert_eq!(FALLOUT4.classify_mod(tmp.path()), ModSafety::SaveBreaking);
}

#[test]
fn bethesda_classify_pex_script_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("myscript.pex"), b"pex").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveBreaking);
}

#[test]
fn bethesda_classify_dll_is_save_breaking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("skse_plugin.dll"), b"MZ").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveBreaking);
}

#[test]
fn bethesda_classify_texture_dds_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("texture.dds"), b"DDS").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn bethesda_classify_mesh_nif_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("helmet.nif"), b"nif").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn bethesda_classify_bsa_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("textures.bsa"), b"BSA").unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn bethesda_classify_ba2_is_save_safe() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("textures.ba2"), b"BA2").unwrap();
    assert_eq!(FALLOUT4.classify_mod(tmp.path()), ModSafety::SaveSafe);
}

#[test]
fn bethesda_classify_empty_dir_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(SKYRIM_SE.classify_mod(tmp.path()), ModSafety::Unknown);
}

// ── ModSafety::affects_saves ──────────────────────────────────────────────────

#[test]
fn mod_safety_affects_saves_semantics() {
    assert!(ModSafety::SaveBreaking.affects_saves());
    assert!(ModSafety::Unknown.affects_saves());
    assert!(!ModSafety::SaveSafe.affects_saves());
}
