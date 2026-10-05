use super::backend::vals;
use super::*;
#[cfg(feature = "postgres")]
use serial_test::serial;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::patcher::{CommandSettings, PatcherStageRow, PatcherStageSettings};

/// Test-only raw query helpers, replacing the previous direct `conn` access.
#[cfg(test)]
impl ModdeDb {
    async fn test_exec(&self, sql: &str, params: &[Val]) -> Result<u64> {
        self.db.execute(sql, params).await
    }

    async fn test_two_i64(&self, sql: &str, params: &[Val]) -> Result<(i64, i64)> {
        self.db
            .fetch_one(sql, params, |r| Ok((r.i64(0)?, r.i64(1)?)))
            .await
    }

    async fn test_three_str(&self, sql: &str, params: &[Val]) -> Result<(String, String, String)> {
        self.db
            .fetch_one(sql, params, |r| {
                Ok((r.string(0)?, r.string(1)?, r.string(2)?))
            })
            .await
    }
}

async fn test_db() -> ModdeDb {
    ModdeDb::open_memory().await.unwrap()
}

#[cfg(feature = "postgres")]
fn env_from(values: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let values: HashMap<&'static str, String> = values
        .iter()
        .map(|(key, value)| (*key, (*value).to_string()))
        .collect();
    move |key| values.get(key).cloned()
}

#[cfg(feature = "postgres")]
fn empty_env(_key: &str) -> Option<String> {
    None
}

#[cfg(feature = "postgres")]
struct EnvRestore {
    key: &'static str,
    previous: Option<String>,
}

#[cfg(feature = "postgres")]
impl EnvRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: this test helper is used only by #[serial] tests that restore
        // the exact variables they mutate before returning.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

#[cfg(feature = "postgres")]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        // SAFETY: the serial test guard restores process environment after a
        // test-scoped mutation and is not shared across concurrently running
        // tests in this module.
        unsafe {
            if let Some(value) = &self.previous {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

fn sample_profile(name: &str, game_id: &str) -> Profile {
    Profile {
        id: None,
        name: name.to_string(),
        game_id: GameId::from(game_id),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "mod_a".to_string(),
                enabled: true,
                version: Some("1.0".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "mod_b".to_string(),
                enabled: false,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_b"),
            after: ModId::from("mod_a"),
        }],
        load_order_lock: None,
    }
}

#[tokio::test]
async fn ui_test_cleanup_clears_sqlite_fixture_state() {
    let db = test_db().await;
    let game = GameId::from("fixture-game");
    db.save_tool_config(&game, "fixture-tool", true, "{}")
        .await
        .unwrap();
    assert!(
        db.load_tool_config(&game, "fixture-tool")
            .await
            .unwrap()
            .is_some()
    );
    db.clear_ui_test_state().await.unwrap();
    assert!(
        db.load_tool_config(&game, "fixture-tool")
            .await
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn ui_test_cleanup_rejects_postgres_before_executing_sql() {
    // A closed lazy pool cannot connect or execute SQL, even in the red test.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://fixture@127.0.0.1:1/fixture")
        .unwrap();
    pool.close().await;
    let db = ModdeDb {
        db: Db::Postgres(pool),
    };
    let error = db.clear_ui_test_state().await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("UI test cleanup requires an isolated SQLite database")
    );
}

mod patcher;
#[cfg(feature = "postgres")]
mod postgres;
mod profile;
