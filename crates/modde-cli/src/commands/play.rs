use anyhow::{Context, Result, bail};

/// Compatibility entry point. Library and CLI launches share the exact-install
/// lifecycle; ambiguous game-only requests must be made explicit.
pub async fn handle(
    profile_name: Option<String>,
    game_id: String,
    no_deploy: bool,
    no_switch: bool,
    no_capture: bool,
) -> Result<()> {
    let catalogue = modde_games::library::catalogue(&modde_core::settings::AppSettings::load())?;
    let matches: Vec<_> = catalogue.games.iter().filter(|game| {
        game.game_id.as_deref() == Some(game_id.as_str()) && game.install_path.is_some()
    }).collect();
    if matches.len() > 1 {
        bail!("multiple installations for {game_id}; use `modde library list` then `modde library play <id>`");
    }
    let game = matches.first().context("no installation found for this game")?;
    super::library::check_outcome(super::library::play(&game.id, super::library::PlayOptions {
        profile: profile_name, no_deploy, no_switch, no_capture,
        ..Default::default()
    }).await?)
}
