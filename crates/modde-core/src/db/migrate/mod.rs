//! Schema creation and migration — the only genuinely per-backend code.
//!
//! **`SQLite`** keeps modde's original `PRAGMA user_version` migration ladder
//! byte-for-byte (`SCHEMA_V1..V10` plus the imperative column adds for the
//! squashed V4–V7 steps), so an existing on-disk database upgrades exactly as
//! it did under `rusqlite`. Do not "modernize" this ladder.
//!
//! **`PostgreSQL`** has no existing databases in the wild, so it gets a single
//! forward-only, fully `IF NOT EXISTS`-idempotent end-state schema (the V10
//! shape), version-tracked in a `schema_version` table. `AUTOINCREMENT` becomes
//! `GENERATED ALWAYS AS IDENTITY`, the `INTEGER`-boolean columns become real
//! `BOOLEAN`, and the timestamp `TEXT` columns keep a `to_char(now(), …)`
//! default so they read back as the same `String` format `SQLite` produces.

use sqlx::AssertSqlSafe;
use tracing::info;

use crate::error::Result;

/// Current schema version. Bump this and add a migration step (`SQLite` ladder +
/// Postgres end-state DDL) when the schema changes.
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 17;

// ── SQLite schema constants (verbatim from the original rusqlite layer) ──────

#[cfg(feature = "postgres")]
mod postgres;
mod sqlite_schema;

#[cfg(feature = "postgres")]
pub(crate) use postgres::migrate_postgres;
use sqlite_schema::{
    SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V8, SCHEMA_V9, SCHEMA_V10, SCHEMA_V11, SCHEMA_V12,
    SCHEMA_V13, SCHEMA_V14, SCHEMA_V15, SCHEMA_V16, SCHEMA_V17,
};

async fn sqlite_user_version(pool: &sqlx::SqlitePool) -> Result<i64> {
    use sqlx::Row;
    let row = sqlx::query("PRAGMA user_version").fetch_one(pool).await?;
    Ok(row.try_get(0)?)
}

async fn sqlite_set_user_version(pool: &sqlx::SqlitePool, version: i64) -> Result<()> {
    // PRAGMA values cannot be bound; `version` is an internal constant.
    sqlx::query(AssertSqlSafe(format!("PRAGMA user_version = {version}")))
        .execute(pool)
        .await?;
    Ok(())
}

async fn sqlite_column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> Result<bool> {
    use sqlx::Row;
    // Identifiers come only from the internal migration ladder below.
    let rows = sqlx::query(AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(pool)
        .await?;
    for row in &rows {
        let name: String = row.try_get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn sqlite_add_column_if_missing(
    pool: &sqlx::SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    if sqlite_column_exists(pool, table, column).await? {
        return Ok(());
    }
    // Table, column, and definition are fixed strings from the migration ladder.
    sqlx::raw_sql(AssertSqlSafe(format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition};"
    )))
    .execute(pool)
    .await?;
    Ok(())
}

/// Apply the `SQLite` migration ladder, preserving the historical `user_version`
/// gates exactly (including the squashed V4/V5 steps).
pub(crate) async fn migrate_sqlite(pool: &sqlx::SqlitePool) -> Result<()> {
    let version = sqlite_user_version(pool).await?;

    if version < 1 {
        sqlx::raw_sql(SCHEMA_V1).execute(pool).await?;
        info!(from = version, to = 1, "database schema migrated to V1");
    }

    if version < 2 {
        sqlx::raw_sql(SCHEMA_V2).execute(pool).await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_mod_id", "INTEGER").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_file_id", "INTEGER").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_game_domain", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "installed_timestamp", "INTEGER")
            .await?;
        sqlite_add_column_if_missing(
            pool,
            "profile_mods",
            "category_id",
            "INTEGER REFERENCES mod_categories(id)",
        )
        .await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "notes", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "tags", "TEXT").await?;
        info!(
            from = version.max(1),
            to = 2,
            "database schema migrated to V2"
        );
    }

    if version < 3 {
        sqlx::raw_sql(SCHEMA_V3).execute(pool).await?;
        info!(
            from = version.max(2),
            to = 3,
            "database schema migrated to V3"
        );
    }

    if version < 6 {
        sqlite_add_column_if_missing(pool, "profile_mods", "display_name", "TEXT").await?;
        info!(
            from = version.max(5),
            to = 6,
            "database schema migrated to V6"
        );
    }

    if version < 7 {
        sqlite_add_column_if_missing(pool, "profiles", "load_order_lock", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "lock_reason", "TEXT").await?;
        info!(
            from = version.max(6),
            to = 7,
            "database schema migrated to V7"
        );
    }

    if version < 8 {
        sqlite_add_column_if_missing(pool, "profile_mods", "install_method", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "source_archive_hash", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "install_status", "TEXT").await?;
        sqlx::raw_sql(SCHEMA_V8).execute(pool).await?;
        info!(
            from = version.max(7),
            to = 8,
            "database schema migrated to V8"
        );
    }

    if version < 9 {
        sqlx::raw_sql(SCHEMA_V9).execute(pool).await?;
        info!(
            from = version.max(8),
            to = 9,
            "database schema migrated to V9"
        );
    }

    if version < 10 {
        sqlx::raw_sql(SCHEMA_V10).execute(pool).await?;
        sqlite_add_column_if_missing(pool, "game_tools", "current_node_id", "TEXT").await?;
        info!(
            from = version.max(9),
            to = 10,
            "database schema migrated to V10"
        );
    }

    if version < 11 {
        sqlx::raw_sql(SCHEMA_V11).execute(pool).await?;
        info!(
            from = version.max(10),
            to = 11,
            "database schema migrated to V11"
        );
    }

    if version < 12 {
        sqlx::raw_sql(SCHEMA_V12).execute(pool).await?;
        info!(
            from = version.max(11),
            to = 12,
            "database schema migrated to V12"
        );
    }

    if version < 13 {
        sqlx::raw_sql(SCHEMA_V13).execute(pool).await?;
        info!(
            from = version.max(12),
            to = 13,
            "database schema migrated to V13"
        );
    }

    if version < 14 {
        sqlx::raw_sql(SCHEMA_V14).execute(pool).await?;
        info!(
            from = version.max(13),
            to = 14,
            "database schema migrated to V14"
        );
    }

    if version < 15 {
        sqlx::raw_sql(SCHEMA_V15).execute(pool).await?;
        info!(
            from = version.max(14),
            to = 15,
            "database schema migrated to V15"
        );
    }

    if version < 16 {
        sqlx::raw_sql(SCHEMA_V16).execute(pool).await?;
        info!(
            from = version.max(15),
            to = 16,
            "database schema migrated to V16"
        );
    }

    if version < 17 {
        sqlx::raw_sql(SCHEMA_V17).execute(pool).await?;
        info!(
            from = version.max(16),
            to = 17,
            "database schema migrated to V17"
        );
    }

    if version < CURRENT_SCHEMA_VERSION {
        sqlite_set_user_version(pool, CURRENT_SCHEMA_VERSION).await?;
    }

    Ok(())
}

// ── PostgreSQL end-state schema ─────────────────────────────────────────────
