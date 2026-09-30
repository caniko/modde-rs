//! Classify the per-mod source tree, before files are merged for deployment.

use std::path::Path;

use modde_core::profile::{Profile, ProfileSource};
use modde_core::save::SaveFingerprint;

/// Fingerprint enabled mods using the same stored contents as deployment.
/// Missing content and unclassified games are conservatively save-affecting.
#[must_use]
pub fn save_fingerprint(profile: &Profile) -> SaveFingerprint {
    fingerprint_at(profile, &modde_core::paths::store_dir())
}

fn fingerprint_at(profile: &Profile, store: &Path) -> SaveFingerprint {
    let plugin = crate::resolve_game_plugin(profile.game_id.as_str());
    SaveFingerprint::compute(&profile.mods, |id| {
        // Wabbajack IDs identify input archives; directive outputs are split
        // across its staging tree and cannot be classified by archive ID.
        if matches!(profile.source, ProfileSource::Wabbajack { .. }) { return true; }
        plugin.is_none_or(|plugin| plugin.classify_mod(&store.join(id)).affects_saves())
    })
}

#[cfg(all(test, feature = "stardew"))]
mod tests {
    use super::*;
    use modde_core::profile::EnabledMod;

    #[test]
    fn source_contents_distinguish_cosmetic_mods_before_any_deployment() {
        let root = tempfile::tempdir().unwrap();
        for (id, file) in [("cosmetic", "portrait.png"), ("script", "extension.dll")] {
            let directory = root.path().join(id);
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join(file), "fixture").unwrap();
        }
        let mut profile = Profile {
            id: None, name: "not-deployed".into(), game_id: "stardew-valley".into(),
            source: ProfileSource::Manual, overrides: root.path().join("overrides"),
            load_order_rules: Default::default(), load_order_lock: None,
            mods: ["cosmetic", "script", "missing"].into_iter().map(|id| EnabledMod {
                mod_id: id.into(), enabled: true, ..Default::default()
            }).collect(),
        };
        let fingerprint = fingerprint_at(&profile, root.path());
        assert_eq!(fingerprint.mod_ids.as_slice(), &["missing", "script"]);
        profile.name = "renamed-profile".into();
        assert_eq!(fingerprint_at(&profile, root.path()), fingerprint);
        profile.mods[1].enabled = false;
        assert_eq!(fingerprint_at(&profile, root.path()).mod_ids.as_slice(), &["missing"]);
        profile.source = ProfileSource::Wabbajack { manifest_hash: "manifest".into() };
        assert_eq!(fingerprint_at(&profile, root.path()).mod_ids.as_slice(), &["cosmetic", "missing"]);
    }
}
