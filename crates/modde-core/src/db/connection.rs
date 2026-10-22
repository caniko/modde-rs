//! Database connection constructors and health checks.
#![allow(clippy::wildcard_imports)]

use super::*;

impl ModdeDb {
    pub async fn open() -> Result<Self> {
        let settings = AppSettings::load();
        Self::open_with_settings(&settings).await
    }

    /// Open the database described by `settings`, honoring the
    /// `MODDE_DATABASE_*` environment overrides used by the Home Manager module.
    pub async fn open_with_settings(settings: &AppSettings) -> Result<Self> {
        let backend = std::env::var("MODDE_DATABASE_BACKEND")
            .ok()
            .and_then(|v| DbBackend::parse(&v))
            .unwrap_or(settings.database.backend);
        match backend {
            DbBackend::Sqlite => Self::open_at(&crate::paths::db_path()).await,
            DbBackend::Postgres => Self::open_postgres(&settings.database).await,
        }
    }

    /// Open a `SQLite` database at a specific path, creating it if needed.
    pub async fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        migrate::migrate_sqlite(&pool).await?;
        Ok(Self {
            db: Db::Sqlite(pool),
        })
    }

    /// Open an in-memory `SQLite` database (for testing).
    pub async fn open_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        migrate::migrate_sqlite(&pool).await?;
        Ok(Self {
            db: Db::Sqlite(pool),
        })
    }

    /// Run the lightest backend-agnostic query available to prove the open
    /// connection can execute SQL.
    pub async fn ping(&self) -> Result<()> {
        self.db
            // Postgres types a bare `1` as INT4; the shared row API decodes INT8.
            .fetch_one("SELECT CAST(1 AS BIGINT)", &vals![], |r| r.i64(0))
            .await?;
        Ok(())
    }

    #[cfg(feature = "postgres")]
    async fn open_postgres(settings: &DatabaseSettings) -> Result<Self> {
        use sqlx::postgres::PgPoolOptions;

        let mut opts = build_pg_options(settings, &|key| std::env::var(key).ok())?;

        let pw_path = std::env::var("MODDE_DB_PASSWORD_FILE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| settings.password_file.clone());
        if let Some(path) = pw_path {
            let pw = read_pg_password_file(&path)?;
            opts = opts.password(&pw);
        }

        let summary = describe_pg_options(&opts);
        let pool = PgPoolOptions::new().connect_with(opts).await.map_err(|e| {
            CoreError::Other(format!("failed to connect to postgres ({summary}): {e}").into())
        })?;
        migrate::migrate_postgres(&pool).await?;
        Ok(Self {
            db: Db::Postgres(pool),
        })
    }

    #[cfg(not(feature = "postgres"))]
    async fn open_postgres(_settings: &DatabaseSettings) -> Result<Self> {
        Err(CoreError::Other(
            "PostgreSQL backend requested but modde was built without the `postgres` feature"
                .into(),
        ))
    }

    // ── Profile CRUD ──────────────────────────────────────────────
}
