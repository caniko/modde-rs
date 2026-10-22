//! Legacy Proton save discovery for Bethesda games. Kept in its own test
//! binary because it points `HOME` at a temporary directory.

#![cfg(all(target_os = "linux", feature = "bethesda"))]

use modde_games::GamePlugin;

#[test]
fn saves_are_found_under_the_lowercase_proton_users_dir() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, so nothing reads HOME concurrently.
    unsafe { std::env::set_var("HOME", home.path()) };
    let game = &modde_games::bethesda::SKYRIM_SE;
    assert_eq!(game.save_directory(), None);

    // Proton always creates `drive_c/users`, in lowercase.
    let saves = home.path().join(
        ".local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/Documents/My Games/Skyrim Special Edition/Saves",
    );
    std::fs::create_dir_all(&saves).unwrap();
    assert_eq!(game.save_directory(), Some(saves));
}
