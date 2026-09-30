//! `modde config` -- inspect, validate, and set modde's configuration,
//! including the database backend.
//!
//! `show` reports the resolved database configuration using the same precedence
//! as runtime startup: environment first, then `settings.toml`, then backend
//! defaults where the backend has them. The honored database environment
//! variables are `MODDE_DATABASE_BACKEND`, `MODDE_DATABASE_URL`,
//! `MODDE_DATABASE_HOST`, `MODDE_DATABASE_PORT`, `MODDE_DATABASE_NAME`,
//! `MODDE_DATABASE_USER`, and `MODDE_DB_PASSWORD_FILE`.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;

use modde_core::db::ModdeDb;
#[cfg(feature = "postgres")]
use modde_core::db::{build_pg_options, describe_pg_options};
use modde_core::settings::{AppSettings, DatabaseSettings, DbBackend};

/// Stored database fields that `config set-database --clear` can unset.
#[derive(Clone, Debug, ValueEnum)]
pub enum ClearDatabaseField {
    Url,
    Host,
    Port,
    Dbname,
    User,
    PasswordFile,
}

#[derive(Clone, Copy)]
enum Source {
    Env(&'static str),
    Settings,
    #[cfg(feature = "postgres")]
    Default,
    #[cfg(feature = "postgres")]
    Unset,
}

impl Source {
    const fn label(self) -> &'static str {
        match self {
            Self::Env(key) => key,
            Self::Settings => "settings.toml",
            #[cfg(feature = "postgres")]
            Self::Default => "backend default",
            #[cfg(feature = "postgres")]
            Self::Unset => "unset",
        }
    }
}

#[cfg(feature = "postgres")]
struct ResolvedField<T> {
    value: T,
    source: Source,
}

fn env_var(key: &'static str) -> Option<String> {
    std::env::var(key).ok()
}

#[cfg(feature = "postgres")]
fn resolved_string(
    env_key: &'static str,
    setting: Option<String>,
    default: Option<&'static str>,
) -> ResolvedField<Option<String>> {
    if let Some(value) = env_var(env_key) {
        return ResolvedField {
            value: Some(value),
            source: Source::Env(env_key),
        };
    }
    if let Some(value) = setting {
        return ResolvedField {
            value: Some(value),
            source: Source::Settings,
        };
    }
    if let Some(value) = default {
        return ResolvedField {
            value: Some(value.to_string()),
            source: Source::Default,
        };
    }
    ResolvedField {
        value: None,
        source: Source::Unset,
    }
}

#[cfg(feature = "postgres")]
fn resolved_path(
    env_key: &'static str,
    setting: Option<PathBuf>,
) -> ResolvedField<Option<PathBuf>> {
    if let Some(value) = env_var(env_key) {
        return ResolvedField {
            value: Some(PathBuf::from(value)),
            source: Source::Env(env_key),
        };
    }
    if let Some(value) = setting {
        return ResolvedField {
            value: Some(value),
            source: Source::Settings,
        };
    }
    ResolvedField {
        value: None,
        source: Source::Unset,
    }
}

#[cfg(feature = "postgres")]
fn resolved_port(setting: Option<u16>) -> Result<ResolvedField<u16>> {
    if let Some(raw) = env_var("MODDE_DATABASE_PORT") {
        let port = raw.parse::<u16>().map_err(|e| {
            anyhow!("invalid MODDE_DATABASE_PORT value '{raw}': expected 0-65535 ({e})")
        })?;
        return Ok(ResolvedField {
            value: port,
            source: Source::Env("MODDE_DATABASE_PORT"),
        });
    }
    if let Some(port) = setting {
        return Ok(ResolvedField {
            value: port,
            source: Source::Settings,
        });
    }
    Ok(ResolvedField {
        value: 5432,
        source: Source::Default,
    })
}

fn print_field(name: &str, value: &str, source: Source) {
    println!("  {name}: {value} (from {})", source.label());
}

#[cfg(feature = "postgres")]
fn redact_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return "<unparseable url; redacted>".to_string();
    };
    if url.password().is_some() {
        let _ = url.set_password(Some("<redacted>"));
    }
    url.to_string()
}

#[cfg(feature = "postgres")]
fn postgres_summary(db: &DatabaseSettings) -> Result<String> {
    let opts = build_pg_options(db, &|key| std::env::var(key).ok())?;
    Ok(describe_pg_options(&opts))
}

#[cfg(not(feature = "postgres"))]
fn postgres_summary(_db: &DatabaseSettings) -> Result<String> {
    bail!("PostgreSQL support requires the postgres feature")
}

/// Print the resolved database configuration and its source.
pub fn handle_show() -> Result<()> {
    let settings = AppSettings::load();
    let db = &settings.database;

    let (effective_backend, backend_source) = match env_var("MODDE_DATABASE_BACKEND") {
        Some(env) => (
            DbBackend::parse(&env)
                .ok_or_else(|| anyhow!("invalid MODDE_DATABASE_BACKEND value '{env}'"))?,
            Source::Env("MODDE_DATABASE_BACKEND"),
        ),
        None => (db.backend, Source::Settings),
    };

    print_field(
        "database backend",
        effective_backend.as_str(),
        backend_source,
    );

    match effective_backend {
        DbBackend::Sqlite => {
            println!("  sqlite path: {}", modde_core::paths::db_path().display());
        }
        #[cfg(not(feature = "postgres"))]
        DbBackend::Postgres => bail!("PostgreSQL support requires the postgres feature"),
        #[cfg(feature = "postgres")]
        DbBackend::Postgres => {
            let url = resolved_string("MODDE_DATABASE_URL", db.url.clone(), None);
            let resolved = build_pg_options(db, &|key| std::env::var(key).ok());
            if let Some(value) = &url.value {
                print_field("url", &redact_url(value), url.source);
                if let Ok(opts) = &resolved {
                    println!("  resolved: {}", describe_pg_options(opts));
                }
            } else {
                let host = resolved_string(
                    "MODDE_DATABASE_HOST",
                    db.host.clone(),
                    Some("<backend default>"),
                );
                let port = resolved_port(db.port)?;
                let dbname = resolved_string("MODDE_DATABASE_NAME", db.dbname.clone(), None);
                let user = resolved_string(
                    "MODDE_DATABASE_USER",
                    db.user.clone(),
                    Some("<backend default>"),
                );

                let resolved_host = resolved
                    .as_ref()
                    .map(|opts| opts.get_host().to_string())
                    .ok()
                    .or(host.value);
                let resolved_port = match resolved.as_ref() {
                    Ok(opts) => opts.get_port(),
                    Err(_) => port.value,
                };
                let resolved_dbname = resolved
                    .as_ref()
                    .ok()
                    .and_then(|opts| opts.get_database())
                    .map(str::to_string)
                    .or(dbname.value);
                let resolved_user = resolved
                    .as_ref()
                    .map(|opts| opts.get_username().to_string())
                    .ok()
                    .or(user.value);

                print_field(
                    "host",
                    resolved_host.as_deref().unwrap_or("<unset>"),
                    host.source,
                );
                print_field("port", &resolved_port.to_string(), port.source);
                print_field(
                    "dbname",
                    resolved_dbname.as_deref().unwrap_or("<unset>"),
                    dbname.source,
                );
                print_field(
                    "user",
                    resolved_user.as_deref().unwrap_or("<unset>"),
                    user.source,
                );
            }

            let pw = resolved_path("MODDE_DB_PASSWORD_FILE", db.password_file.clone());
            let pw_value = pw
                .value
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string());
            print_field("password file", &pw_value, pw.source);

            if let Err(e) = resolved {
                println!("  resolution error: {e}");
            }
        }
    }

    println!("config file: {}", AppSettings::config_path().display());
    Ok(())
}

/// Open the resolved database, run a trivial query, and report the result.
pub async fn handle_test() -> Result<()> {
    let settings = AppSettings::load();
    let db = &settings.database;
    let backend = env_var("MODDE_DATABASE_BACKEND")
        .as_deref()
        .and_then(DbBackend::parse)
        .unwrap_or(db.backend);

    println!("database backend: {}", backend.as_str());
    match backend {
        DbBackend::Sqlite => {
            println!("summary: path={}", modde_core::paths::db_path().display());
        }
        DbBackend::Postgres => {
            println!("summary: {}", postgres_summary(db)?);
        }
    }

    let db = ModdeDb::open()
        .await
        .with_context(|| format!("failed to open {} database", backend.as_str()))?;
    db.ping()
        .await
        .with_context(|| format!("failed to run database probe against {}", backend.as_str()))?;
    println!("OK");
    Ok(())
}

/// Reset stored database settings to `SQLite` and clear all `PostgreSQL` fields.
pub fn handle_reset_database() -> Result<()> {
    let mut settings = AppSettings::load();
    settings.database.backend = DbBackend::Sqlite;
    clear_postgres_fields(&mut settings.database);
    settings.save();
    println!("database backend reset to sqlite");
    println!("postgres connection fields cleared from settings.toml");
    Ok(())
}

fn clear_postgres_fields(db: &mut DatabaseSettings) {
    db.url = None;
    db.host = None;
    db.port = None;
    db.dbname = None;
    db.user = None;
    db.password_file = None;
}

fn apply_clears(db: &mut DatabaseSettings, clear: &[ClearDatabaseField]) {
    for field in clear {
        match field {
            ClearDatabaseField::Url => db.url = None,
            ClearDatabaseField::Host => db.host = None,
            ClearDatabaseField::Port => db.port = None,
            ClearDatabaseField::Dbname => db.dbname = None,
            ClearDatabaseField::User => db.user = None,
            ClearDatabaseField::PasswordFile => db.password_file = None,
        }
    }
}

enum FieldUpdate<T> {
    Unchanged,
    Clear,
    Set(T),
}

fn normalize_string(value: Option<String>) -> FieldUpdate<String> {
    value
        .map(|s| {
            if s.trim().is_empty() {
                FieldUpdate::Clear
            } else {
                FieldUpdate::Set(s)
            }
        })
        .unwrap_or(FieldUpdate::Unchanged)
}

fn normalize_path(value: Option<PathBuf>) -> FieldUpdate<PathBuf> {
    value
        .map(|path| {
            if path.as_os_str().is_empty() {
                FieldUpdate::Clear
            } else {
                FieldUpdate::Set(path)
            }
        })
        .unwrap_or(FieldUpdate::Unchanged)
}

fn has_discrete(db: &DatabaseSettings) -> bool {
    db.host.is_some() || db.port.is_some() || db.dbname.is_some() || db.user.is_some()
}

fn validate_database(db: &DatabaseSettings) -> Result<()> {
    match db.backend {
        DbBackend::Sqlite => {
            if db.url.is_some()
                || db.host.is_some()
                || db.port.is_some()
                || db.dbname.is_some()
                || db.user.is_some()
                || db.password_file.is_some()
            {
                bail!(
                    "connection fields (url/host/port/name/user/password-file) are only valid when backend = postgres"
                );
            }
        }
        DbBackend::Postgres => {
            if db.url.is_none() && db.dbname.is_none() {
                bail!(
                    "backend = postgres requires either --url, or at least --name (host/port/user optional, default to the local socket)"
                );
            }
            if db.url.is_some() && has_discrete(db) {
                bail!("set --url OR the discrete --host/--port/--name/--user fields, not both");
            }
        }
    }
    Ok(())
}

/// Update the stored database backend and `PostgreSQL` connection parameters.
#[allow(clippy::too_many_arguments)]
pub fn handle_set_database(
    backend: &str,
    url: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    dbname: Option<String>,
    user: Option<String>,
    password_file: Option<PathBuf>,
    clear: &[ClearDatabaseField],
) -> Result<()> {
    let backend = DbBackend::parse(backend)
        .ok_or_else(|| anyhow!("invalid backend '{backend}' (expected 'sqlite' or 'postgres')"))?;

    let mut settings = AppSettings::load();
    settings.database.backend = backend;

    if backend == DbBackend::Sqlite {
        clear_postgres_fields(&mut settings.database);
    } else {
        apply_clears(&mut settings.database, clear);
    }

    match normalize_string(url) {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear => settings.database.url = None,
        FieldUpdate::Set(value) => settings.database.url = Some(value),
    }
    match normalize_string(host) {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear => settings.database.host = None,
        FieldUpdate::Set(value) => settings.database.host = Some(value),
    }
    if let Some(value) = port {
        settings.database.port = Some(value);
    }
    match normalize_string(dbname) {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear => settings.database.dbname = None,
        FieldUpdate::Set(value) => settings.database.dbname = Some(value),
    }
    match normalize_string(user) {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear => settings.database.user = None,
        FieldUpdate::Set(value) => settings.database.user = Some(value),
    }
    match normalize_path(password_file) {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear => settings.database.password_file = None,
        FieldUpdate::Set(value) => settings.database.password_file = Some(value),
    }

    validate_database(&settings.database)?;
    settings.save();

    println!("database backend set to {}", backend.as_str());
    if backend == DbBackend::Sqlite {
        println!("postgres connection fields cleared from settings.toml");
    } else {
        println!(
            "note: the PostgreSQL password is read at runtime from the configured password file; \
             it is never stored in settings.toml."
        );
    }
    Ok(())
}
