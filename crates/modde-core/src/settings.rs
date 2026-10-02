use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::resolver::GameId;

/// Persistent application settings shared between CLI and UI.
///
/// Stored as TOML at `<config_dir>/modde/settings.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nexus_api_key: String,
    /// Configured game install paths — typically 1–4 games.
    /// `SmallVec<[_; 4]>` keeps ≤4 entries inline (no heap allocation).
    #[serde(default)]
    pub game_paths: SmallVec<[GamePath; 4]>,
    #[serde(default)]
    pub download_dir: Option<PathBuf>,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub selected_game: Option<String>,
    #[serde(default)]
    pub update_check: UpdateCheckSettings,
    /// Storage backend selection. Defaults to `SQLite`; omitted from old
    /// `settings.toml` files, which therefore keep using `SQLite` unchanged.
    #[serde(default)]
    pub database: DatabaseSettings,
    #[serde(default)]
    pub doctor: DoctorSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DoctorSettings {
    #[serde(default)]
    pub llm: DoctorLlmSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorLlmSettings {
    #[serde(default = "default_local_llm_endpoint")]
    pub local_endpoint: String,
    #[serde(default = "default_local_llm_model")]
    pub local_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_model: Option<String>,
    #[serde(default = "default_remote_api_key_env")]
    pub remote_api_key_env: String,
    #[serde(default = "default_doctor_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_doctor_max_context_bytes")]
    pub max_context_bytes: usize,
}

impl Default for DoctorLlmSettings {
    fn default() -> Self {
        Self {
            local_endpoint: default_local_llm_endpoint(),
            local_model: default_local_llm_model(),
            remote_endpoint: None,
            remote_model: None,
            remote_api_key_env: default_remote_api_key_env(),
            timeout_seconds: default_doctor_timeout_seconds(),
            max_context_bytes: default_doctor_max_context_bytes(),
        }
    }
}

fn default_local_llm_endpoint() -> String {
    "http://127.0.0.1:8080/v1".to_string()
}

fn default_local_llm_model() -> String {
    "local-model".to_string()
}

fn default_remote_api_key_env() -> String {
    "MODDE_DOCTOR_REMOTE_API_KEY".to_string()
}

const fn default_doctor_timeout_seconds() -> u64 {
    120
}

const fn default_doctor_max_context_bytes() -> usize {
    96 * 1024
}

/// Which database backend modde stores its state in, plus the `PostgreSQL`
/// connection parameters (used only when `backend = "postgres"`).
///
/// Every field is `#[serde(default)]`, so existing config files round-trip and
/// default to `SQLite`. Environment variables override these at startup
/// (`MODDE_DATABASE_BACKEND`, `MODDE_DATABASE_URL`, the discrete
/// `MODDE_DATABASE_HOST`/`PORT`/`NAME`/`USER` fields, and
/// `MODDE_DB_PASSWORD_FILE`) so the Home Manager module can configure the
/// backend declaratively without rewriting `settings.toml`. The `PostgreSQL`
/// password is never stored here — it is read at runtime from `password_file`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseSettings {
    #[serde(default)]
    pub backend: DbBackend,
    /// Full connection URL (e.g. `postgres://user@host/db`). Takes precedence
    /// over the discrete `host`/`port`/`dbname`/`user` fields when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Database name. The matching environment variable is
    /// `MODDE_DATABASE_NAME`, not `MODDE_DATABASE_DBNAME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Path to a file containing the `PostgreSQL` password (sops-nix compatible).
    /// Read at runtime; never written back to `settings.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_file: Option<PathBuf>,
}

/// Selectable storage backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbBackend {
    #[default]
    Sqlite,
    Postgres,
}

impl DbBackend {
    /// Parse a backend name (used for the `MODDE_DATABASE_BACKEND` env var and
    /// the CLI `--backend` flag). Case-insensitive.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" | "pg" => Some(Self::Postgres),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckSettings {
    #[serde(default = "default_update_check_enabled")]
    pub enabled: bool,
}

const fn default_update_check_enabled() -> bool {
    true
}

impl Default for UpdateCheckSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePath {
    pub game_id: GameId,
    pub path: PathBuf,
}

impl AppSettings {
    /// Path to the `settings.toml` file backing [`AppSettings`].
    #[must_use]
    pub fn config_path() -> PathBuf {
        crate::paths::modde_config_dir().join("settings.toml")
    }

    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(error = %e, "failed to create config directory");
        }
        if let Ok(s) = toml::to_string_pretty(self)
            && let Err(e) = std::fs::write(&path, s)
        {
            tracing::warn!(error = %e, "failed to write settings file");
        }
    }

    /// Get the install path for a game, if configured.
    #[must_use]
    pub fn game_path(&self, game_id: &GameId) -> Option<&PathBuf> {
        self.game_paths
            .iter()
            .find(|gp| gp.game_id == *game_id)
            .map(|gp| &gp.path)
    }

    /// Set the install path for a game (add or update).
    pub fn set_game_path(&mut self, game_id: &GameId, path: PathBuf) {
        if let Some(entry) = self.game_paths.iter_mut().find(|gp| gp.game_id == *game_id) {
            entry.path = path;
        } else {
            self.game_paths.push(GamePath {
                game_id: game_id.clone(),
                path,
            });
        }
    }

    /// Load settings from a specific file path.
    #[must_use]
    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save settings to a specific file path.
    pub fn save_to(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(error = %e, "failed to create config directory");
        }
        if let Ok(s) = toml::to_string_pretty(self)
            && let Err(e) = std::fs::write(path, s)
        {
            tracing::warn!(error = %e, "failed to write settings file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_settings_are_empty() {
        let s = AppSettings::default();
        assert_eq!(s.nexus_api_key, "");
        assert!(s.game_paths.is_empty());
        assert!(s.download_dir.is_none());
        assert_eq!(s.theme, "");
        assert!(s.selected_game.is_none());
    }

    #[test]
    fn game_path_lookup_and_set() {
        let mut s = AppSettings::default();
        assert!(s.game_path(&GameId::from("cyberpunk2077")).is_none());

        s.set_game_path(
            &GameId::from("cyberpunk2077"),
            PathBuf::from("/games/cp2077"),
        );
        assert_eq!(
            s.game_path(&GameId::from("cyberpunk2077")),
            Some(&PathBuf::from("/games/cp2077"))
        );

        // Update existing
        s.set_game_path(&GameId::from("cyberpunk2077"), PathBuf::from("/new/path"));
        assert_eq!(
            s.game_path(&GameId::from("cyberpunk2077")),
            Some(&PathBuf::from("/new/path"))
        );
        assert_eq!(s.game_paths.len(), 1, "should update, not duplicate");
    }

    #[test]
    fn multiple_game_paths() {
        let mut s = AppSettings::default();
        s.set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
        s.set_game_path(
            &GameId::from("cyberpunk2077"),
            PathBuf::from("/games/cp2077"),
        );

        assert_eq!(s.game_paths.len(), 2);
        assert_eq!(
            s.game_path(&GameId::from("skyrim-se")),
            Some(&PathBuf::from("/games/skyrim"))
        );
        assert_eq!(
            s.game_path(&GameId::from("cyberpunk2077")),
            Some(&PathBuf::from("/games/cp2077"))
        );
        assert!(s.game_path(&GameId::from("fallout4")).is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");

        let mut original = AppSettings {
            nexus_api_key: "test-key-123".into(),
            ..AppSettings::default()
        };
        original.set_game_path(
            &GameId::from("cyberpunk2077"),
            PathBuf::from("/games/cp2077"),
        );
        original.set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
        original.selected_game = Some("cyberpunk2077".into());
        original.theme = "Nord".into();
        original.download_dir = Some(PathBuf::from("/downloads"));

        original.save_to(&path);

        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded.nexus_api_key, "test-key-123");
        assert_eq!(loaded.selected_game.as_deref(), Some("cyberpunk2077"));
        assert_eq!(loaded.theme, "Nord");
        assert_eq!(loaded.download_dir, Some(PathBuf::from("/downloads")));
        assert_eq!(
            loaded.game_path(&GameId::from("cyberpunk2077")),
            Some(&PathBuf::from("/games/cp2077"))
        );
        assert_eq!(
            loaded.game_path(&GameId::from("skyrim-se")),
            Some(&PathBuf::from("/games/skyrim"))
        );
    }

    #[test]
    fn load_missing_file_returns_default() {
        let s = AppSettings::load_from(Path::new("/nonexistent/settings.toml"));
        assert_eq!(s.nexus_api_key, "");
        assert!(s.game_paths.is_empty());
    }

    #[test]
    fn load_partial_toml_fills_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        std::fs::write(&path, "nexus_api_key = \"mykey\"\n").unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.nexus_api_key, "mykey");
        assert!(s.game_paths.is_empty());
        assert!(s.selected_game.is_none());
    }

    #[test]
    fn missing_database_table_defaults_to_sqlite() {
        let settings: AppSettings = toml::from_str(
            r#"
            nexus_api_key = "legacy-key"
            selected_game = "skyrim-se"
            "#,
        )
        .unwrap();

        assert_eq!(settings.database.backend, DbBackend::Sqlite);
        assert!(settings.database.url.is_none());
        assert!(settings.database.host.is_none());
        assert!(settings.database.port.is_none());
        assert!(settings.database.dbname.is_none());
        assert!(settings.database.user.is_none());
        assert!(settings.database.password_file.is_none());
    }

    #[test]
    fn db_backend_parse_accepts_aliases_and_round_trips() {
        assert_eq!(DbBackend::parse("SQLite"), Some(DbBackend::Sqlite));
        assert_eq!(DbBackend::parse("POSTGRES"), Some(DbBackend::Postgres));
        assert_eq!(DbBackend::parse(" pg "), Some(DbBackend::Postgres));
        assert_eq!(DbBackend::parse("postgresql"), Some(DbBackend::Postgres));
        assert_eq!(DbBackend::parse("mysql"), None);

        for backend in [DbBackend::Sqlite, DbBackend::Postgres] {
            assert_eq!(DbBackend::parse(backend.as_str()), Some(backend));
        }
    }

    #[test]
    fn database_settings_round_trip_preserves_set_fields_and_skips_none() {
        let settings = DatabaseSettings {
            backend: DbBackend::Postgres,
            url: None,
            host: Some("db.example.test".to_string()),
            port: Some(15432),
            dbname: Some("modde".to_string()),
            user: Some("modde_user".to_string()),
            password_file: Some(PathBuf::from("/run/secrets/modde-db-password")),
        };

        let toml = toml::to_string(&settings).unwrap();

        assert!(toml.contains("backend = \"postgres\""));
        assert!(toml.contains("host = \"db.example.test\""));
        assert!(toml.contains("port = 15432"));
        assert!(toml.contains("dbname = \"modde\""));
        assert!(toml.contains("user = \"modde_user\""));
        assert!(toml.contains("password_file = \"/run/secrets/modde-db-password\""));
        assert!(!toml.contains("url ="));

        let loaded: DatabaseSettings = toml::from_str(&toml).unwrap();
        assert_eq!(loaded.backend, settings.backend);
        assert_eq!(loaded.url, settings.url);
        assert_eq!(loaded.host, settings.host);
        assert_eq!(loaded.port, settings.port);
        assert_eq!(loaded.dbname, settings.dbname);
        assert_eq!(loaded.user, settings.user);
        assert_eq!(loaded.password_file, settings.password_file);
    }

    #[test]
    fn load_with_unknown_fields_does_not_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        std::fs::write(
            &path,
            "nexus_api_key = \"key\"\nunknown_field = \"value\"\n",
        )
        .unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.nexus_api_key, "key");
    }

    #[test]
    fn toml_format_is_human_readable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");

        let mut s = AppSettings::default();
        s.set_game_path(
            &GameId::from("cyberpunk2077"),
            PathBuf::from("/games/cp2077"),
        );
        s.selected_game = Some("cyberpunk2077".into());
        s.save_to(&path);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("selected_game"));
        assert!(content.contains("cyberpunk2077"));
        assert!(content.contains("game_id"));
    }
}
