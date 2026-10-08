#![allow(clippy::wildcard_imports)]
use std::collections::HashSet;
use std::fmt;

use iced::widget::pick_list;
use iced::{Element, Length};
use modde_sources::wabbajack::catalog::{CatalogEntrySource, WabbajackCatalogEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOption {
    pub value: String,
    label: String,
}

impl GameOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    pub fn from_game_id(game_id: impl Into<String>) -> Self {
        let value = game_id.into();
        let label = human_game_label(&value);
        Self { value, label }
    }

    pub fn from_wabbajack_game(game: &str) -> Self {
        let value = modde_games::normalize_wabbajack_game(game)
            .map(str::to_string)
            .unwrap_or_else(|| game.trim().to_ascii_lowercase());
        let label = human_game_label(game);
        Self { value, label }
    }
}

impl fmt::Display for GameOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn game_pick_list<'a, Message>(
    options: Vec<GameOption>,
    selected: Option<GameOption>,
    placeholder: &'static str,
    on_selected: impl Fn(GameOption) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let width = pick_list_width(&options, placeholder);
    pick_list(options, selected, on_selected)
        .placeholder(placeholder)
        .width(width)
        .into()
}

pub fn supported_game_options<'a>(
    games: impl IntoIterator<Item = &'a (String, String)>,
) -> Vec<GameOption> {
    games
        .into_iter()
        .map(|(id, _)| GameOption::from_game_id(id.clone()))
        .collect()
}

pub fn nexus_game_options<'a>(
    games: impl IntoIterator<Item = &'a (String, String)>,
) -> Vec<GameOption> {
    games
        .into_iter()
        .filter(|(id, _)| {
            modde_games::resolve_game(id)
                .is_some_and(|game| game.nexus_domain.is_some() && game.nexus_game_id.is_some())
        })
        .map(|(id, _)| GameOption::from_game_id(id.clone()))
        .collect()
}

pub fn supported_game_options_ordered<'a>(
    games: impl IntoIterator<Item = &'a (String, String)>,
    detected_game_ids: &HashSet<String>,
) -> Vec<GameOption> {
    let mut options: Vec<GameOption> = games
        .into_iter()
        .map(|(id, _)| GameOption::from_game_id(id.clone()))
        .collect();
    options.sort_by(|a, b| {
        let a_undetected = !detected_game_ids.contains(&a.value);
        let b_undetected = !detected_game_ids.contains(&b.value);
        a_undetected
            .cmp(&b_undetected)
            .then_with(|| a.to_string().cmp(&b.to_string()))
    });
    options
}

pub fn wabbajack_game_options(
    entries: &[WabbajackCatalogEntry],
    source: &CatalogEntrySource,
) -> Vec<GameOption> {
    let mut options: Vec<GameOption> = entries
        .iter()
        .filter(|entry| &entry.source == source)
        .filter_map(|entry| entry.game.as_deref())
        .map(GameOption::from_wabbajack_game)
        .collect();
    options.sort_by_key(std::string::ToString::to_string);
    options.dedup_by(|a, b| a.value == b.value);
    options
}

pub fn human_game_label(game: &str) -> String {
    let game_id = modde_games::normalize_wabbajack_game(game).unwrap_or(game);
    modde_games::resolve_game_plugin(game_id)
        .map(|plugin| plugin.display_name().to_string())
        .or_else(|| known_wabbajack_game_label(game_id))
        .unwrap_or_else(|| titleize_game_name(game))
}

pub(crate) fn pick_list_width(options: &[GameOption], placeholder: &str) -> Length {
    let longest = options
        .iter()
        .map(|option| option.label.chars().count())
        .chain(std::iter::once(placeholder.chars().count()))
        .max()
        .unwrap_or(0);
    let width = (longest as f32 * 8.0 + 58.0).max(160.0);
    Length::Fixed(width)
}

fn titleize_game_name(game: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut prev_lowercase = false;

    for ch in game.trim().chars() {
        if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lowercase = false;
            continue;
        }

        if ch.is_ascii_uppercase() && prev_lowercase && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }

        current.push(ch);
        prev_lowercase = ch.is_ascii_lowercase();
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn known_wabbajack_game_label(game: &str) -> Option<String> {
    let key: String = game
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();

    let label = match key.as_str() {
        "fallout3" => "Fallout 3",
        "falloutnewvegas" | "newvegas" => "Fallout: New Vegas",
        "morrowind" => "Morrowind",
        "oblivion" => "Oblivion",
        "oblivionremastered" => "Oblivion Remastered",
        "skyrim" => "The Elder Scrolls V: Skyrim",
        "enderal" | "enderalspecialedition" => "Enderal Special Edition",
        _ => return None,
    };
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wabbajack_entry(game: &str, source: CatalogEntrySource) -> WabbajackCatalogEntry {
        WabbajackCatalogEntry {
            title: format!("{game} list"),
            game: Some(game.to_string()),
            author: None,
            version: None,
            tags: Vec::new(),
            image_url: None,
            readme_url: None,
            download_url: format!("https://example/{game}.wabbajack"),
            repository_name: None,
            machine_url: None,
            discord_url: None,
            website_url: None,
            official: true,
            nsfw: false,
            force_down: false,
            size: Default::default(),
            source,
        }
    }

    #[test]
    fn pick_list_width_fits_longest_game_label() {
        let options = vec![
            GameOption::new("short", "Short"),
            GameOption::new(
                "long",
                "The Elder Scrolls V Skyrim Special Edition Anniversary Edition",
            ),
        ];

        let Length::Fixed(width) = pick_list_width(&options, "Select a game") else {
            panic!("expected fixed pick list width");
        };

        assert!(width > 360.0);
    }

    #[test]
    fn wabbajack_game_options_include_supported_and_unsupported_games() {
        let entries = vec![
            wabbajack_entry("skyrimspecialedition", CatalogEntrySource::Official),
            wabbajack_entry("oblivionremastered", CatalogEntrySource::Official),
            wabbajack_entry("morrowind", CatalogEntrySource::Official),
            wabbajack_entry("fallout4", CatalogEntrySource::Authored),
        ];

        let options = wabbajack_game_options(&entries, &CatalogEntrySource::Official);
        let labels: Vec<String> = options.iter().map(ToString::to_string).collect();

        let skyrim_label = if cfg!(feature = "bethesda") {
            "The Elder Scrolls V: Skyrim Special Edition"
        } else {
            "Skyrimspecialedition"
        };
        let oblivion_label = if cfg!(feature = "oblivion-remastered") {
            "The Elder Scrolls IV: Oblivion Remastered"
        } else {
            "Oblivion Remastered"
        };
        assert!(labels.contains(&skyrim_label.to_string()));
        assert!(labels.contains(&oblivion_label.to_string()));
        assert!(labels.contains(&"Morrowind".to_string()));
        assert!(!labels.contains(&"Fallout 4".to_string()));
    }

    #[test]
    fn wabbajack_game_options_dedupe_by_filter_value() {
        let entries = vec![
            wabbajack_entry("SkyrimSpecialEdition", CatalogEntrySource::Official),
            wabbajack_entry("skyrimspecialedition", CatalogEntrySource::Official),
            wabbajack_entry("OblivionRemastered", CatalogEntrySource::Official),
            wabbajack_entry("oblivionremastered", CatalogEntrySource::Official),
        ];

        let options = wabbajack_game_options(&entries, &CatalogEntrySource::Official);
        let skyrim_value = if cfg!(feature = "bethesda") {
            "skyrim-se"
        } else {
            "skyrimspecialedition"
        };
        let oblivion_value = if cfg!(feature = "oblivion-remastered") {
            "oblivion-remastered"
        } else {
            "oblivionremastered"
        };

        assert_eq!(
            options
                .iter()
                .filter(|option| option.value == skyrim_value)
                .count(),
            1
        );
        assert_eq!(
            options
                .iter()
                .filter(|option| option.value == oblivion_value)
                .count(),
            1
        );
    }

    #[test]
    fn supported_game_options_put_undetected_games_last() {
        let games = [
            ("missing-game".to_string(), "Missing".to_string()),
            ("skyrim-se".to_string(), "Skyrim".to_string()),
            ("cyberpunk2077".to_string(), "Cyberpunk".to_string()),
        ];
        let detected = HashSet::from(["skyrim-se".to_string(), "cyberpunk2077".to_string()]);

        let options = supported_game_options_ordered(games.iter(), &detected);
        let values: Vec<&str> = options.iter().map(|option| option.value.as_str()).collect();

        assert_eq!(values, vec!["cyberpunk2077", "skyrim-se", "missing-game"]);
    }

    #[test]
    fn nexus_game_options_only_include_games_with_nexus_domains() {
        let games = [
            ("skyrim-se".to_string(), "Skyrim SE".to_string()),
            ("fallout4".to_string(), "Fallout 4".to_string()),
            ("stellar-blade".to_string(), "Stellar Blade".to_string()),
        ];

        let options = nexus_game_options(games.iter());
        let values: Vec<&str> = options.iter().map(|option| option.value.as_str()).collect();

        assert_eq!(values.contains(&"skyrim-se"), cfg!(feature = "bethesda"));
        assert_eq!(values.contains(&"fallout4"), cfg!(feature = "bethesda"));
        assert!(!values.contains(&"stellar-blade"));
    }
}
