//! Backend-agnostic async SQL executor shared by every [`ModdeDb`](super::ModdeDb)
//! method.
//!
//! modde supports two storage backends — `SQLite` (always available) and
//! `PostgreSQL` (behind the default-on `postgres` feature). Rather than write two
//! copies of every query, the ~60 CRUD methods are authored **once** against
//! this thin executor: SQL is written with `?` placeholders and a small set of
//! portable constructs (`RETURNING id`, `ON CONFLICT … DO UPDATE … EXCLUDED`,
//! `lower(name)`, `bool` columns) that both `sqlx`'s bundled `SQLite` (3.46+) and
//! `PostgreSQL` understand. Only the schema/migration DDL genuinely differs
//! between backends; that lives in [`super::migrate`].
//!
//! The executor bridges the two real divergences:
//!
//! * **Placeholders** — Postgres needs `$1, $2, …`; the `SQLite` `?` form is
//!   rewritten on the way out via [`rewrite_placeholders`].
//! * **`now()`** — the `{NOW}` token expands to `datetime('now')` on `SQLite` and
//!   to a matching `to_char(now(), …)` TEXT expression on Postgres, so the
//!   timestamp columns stay `TEXT` and the Rust struct fields stay `String`.

use std::borrow::Cow;

use sqlx::sqlite::{SqliteArguments, SqlitePool, SqliteRow};
use sqlx::{Arguments, AssertSqlSafe, Row};

#[cfg(feature = "postgres")]
use sqlx::postgres::{PgArguments, PgPool, PgRow};

use crate::error::{CoreError, Result};
use crate::resolver::{GameId, ModId};

/// A backend-agnostic bound parameter.
///
/// Nulls are *typed* (`NullText` / `NullI64`) so `PostgreSQL` — which, unlike
/// `SQLite`, will not infer the type of a bare `NULL` parameter in every position
/// — always receives a parameter with a concrete type.
#[derive(Debug, Clone)]
pub enum Val {
    NullText,
    NullI64,
    NullF64,
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
}

impl From<&str> for Val {
    fn from(s: &str) -> Self {
        Val::Text(s.to_string())
    }
}
impl From<String> for Val {
    fn from(s: String) -> Self {
        Val::Text(s)
    }
}
impl From<&String> for Val {
    fn from(s: &String) -> Self {
        Val::Text(s.clone())
    }
}
impl From<i64> for Val {
    fn from(v: i64) -> Self {
        Val::I64(v)
    }
}
impl From<f64> for Val {
    fn from(v: f64) -> Self {
        Val::F64(v)
    }
}
impl From<bool> for Val {
    fn from(v: bool) -> Self {
        Val::Bool(v)
    }
}
impl From<&GameId> for Val {
    fn from(v: &GameId) -> Self {
        Val::Text(v.as_str().to_string())
    }
}
impl From<&ModId> for Val {
    fn from(v: &ModId) -> Self {
        Val::Text(v.as_str().to_string())
    }
}
impl From<Option<String>> for Val {
    fn from(v: Option<String>) -> Self {
        v.map_or(Val::NullText, Val::Text)
    }
}
impl From<Option<&str>> for Val {
    fn from(v: Option<&str>) -> Self {
        v.map_or(Val::NullText, |s| Val::Text(s.to_string()))
    }
}
impl From<Option<i64>> for Val {
    fn from(v: Option<i64>) -> Self {
        v.map_or(Val::NullI64, Val::I64)
    }
}
impl From<Option<f64>> for Val {
    fn from(v: Option<f64>) -> Self {
        v.map_or(Val::NullF64, Val::F64)
    }
}

/// Build a parameter list for an executor call.
///
/// Expands to a fixed-size `[Val; N]` array (not a `Vec`, so no heap
/// allocation and no `clippy::useless_vec` at the `&vals![..]` call sites).
/// Each argument is converted into a [`Val`] via [`Into`], so `i64`, `bool`,
/// `&str`, `String`, `&GameId`, `&ModId`, and the `Option<…>` variants can be
/// passed directly; callers pass `&vals![..]`, which coerces to `&[Val]`.
macro_rules! vals {
    () => { [] };
    ($($x:expr),+ $(,)?) => { [ $( $crate::db::backend::Val::from($x) ),+ ] };
}
pub(crate) use vals;

/// Column accessor presented to row-mapping closures.
///
/// Implemented for both `SqliteRow` and `PgRow`, this lets every mapper be
/// written once against `&dyn DbRow` with no `sqlx` trait bounds leaking into
/// the call site.
pub trait DbRow {
    fn i64(&self, idx: usize) -> Result<i64>;
    fn opt_i64(&self, idx: usize) -> Result<Option<i64>>;
    fn opt_f64(&self, idx: usize) -> Result<Option<f64>>;
    fn string(&self, idx: usize) -> Result<String>;
    fn opt_string(&self, idx: usize) -> Result<Option<String>>;
    fn bool(&self, idx: usize) -> Result<bool>;
}

impl DbRow for SqliteRow {
    fn i64(&self, idx: usize) -> Result<i64> {
        Ok(self.try_get(idx)?)
    }
    fn opt_i64(&self, idx: usize) -> Result<Option<i64>> {
        Ok(self.try_get(idx)?)
    }
    fn opt_f64(&self, idx: usize) -> Result<Option<f64>> {
        Ok(self.try_get(idx)?)
    }
    fn string(&self, idx: usize) -> Result<String> {
        Ok(self.try_get(idx)?)
    }
    fn opt_string(&self, idx: usize) -> Result<Option<String>> {
        Ok(self.try_get(idx)?)
    }
    fn bool(&self, idx: usize) -> Result<bool> {
        Ok(self.try_get(idx)?)
    }
}

#[cfg(feature = "postgres")]
impl DbRow for PgRow {
    fn i64(&self, idx: usize) -> Result<i64> {
        Ok(self.try_get(idx)?)
    }
    fn opt_i64(&self, idx: usize) -> Result<Option<i64>> {
        Ok(self.try_get(idx)?)
    }
    fn opt_f64(&self, idx: usize) -> Result<Option<f64>> {
        Ok(self.try_get(idx)?)
    }
    fn string(&self, idx: usize) -> Result<String> {
        Ok(self.try_get(idx)?)
    }
    fn opt_string(&self, idx: usize) -> Result<Option<String>> {
        Ok(self.try_get(idx)?)
    }
    fn bool(&self, idx: usize) -> Result<bool> {
        Ok(self.try_get(idx)?)
    }
}

/// Which SQL dialect a connection speaks, used to shape the query text.
#[derive(Clone, Copy)]
enum Dialect {
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
}

fn bind_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Other(format!("failed to bind SQL parameter: {e}").into())
}

fn sqlite_args(vals: &[Val]) -> Result<SqliteArguments> {
    let mut args = SqliteArguments::default();
    for v in vals {
        match v {
            Val::NullText => args.add(None::<String>),
            Val::NullI64 => args.add(None::<i64>),
            Val::NullF64 => args.add(None::<f64>),
            Val::Bool(b) => args.add(*b),
            Val::I64(i) => args.add(*i),
            Val::F64(f) => args.add(*f),
            Val::Text(s) => args.add(s.clone()),
        }
        .map_err(bind_err)?;
    }
    Ok(args)
}

#[cfg(feature = "postgres")]
fn pg_args(vals: &[Val]) -> Result<PgArguments> {
    let mut args = PgArguments::default();
    for v in vals {
        match v {
            Val::NullText => args.add(None::<String>),
            Val::NullI64 => args.add(None::<i64>),
            Val::NullF64 => args.add(None::<f64>),
            Val::Bool(b) => args.add(*b),
            Val::I64(i) => args.add(*i),
            Val::F64(f) => args.add(*f),
            Val::Text(s) => args.add(s.clone()),
        }
        .map_err(bind_err)?;
    }
    Ok(args)
}

/// Rewrite SQLite-style `?` placeholders into `PostgreSQL`'s positional
/// `$1, $2, …` form.
///
/// Single-quoted string literals are skipped so a `?` inside a literal (we have
/// none today, but be safe) is left untouched.
#[cfg(feature = "postgres")]
fn rewrite_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0u32;
    let mut in_str = false;
    for c in sql.chars() {
        match c {
            '\'' => {
                in_str = !in_str;
                out.push(c);
            }
            '?' if !in_str => {
                n += 1;
                out.push('$');
                out.push_str(&n.to_string());
            }
            _ => out.push(c),
        }
    }
    out
}

fn prepare_sql(dialect: Dialect, sql: &str) -> AssertSqlSafe<String> {
    let sql = match dialect {
        Dialect::Sqlite => {
            if sql.contains("{NOW}") {
                Cow::Owned(sql.replace("{NOW}", "datetime('now')"))
            } else {
                Cow::Borrowed(sql)
            }
        }
        #[cfg(feature = "postgres")]
        Dialect::Postgres => {
            let sql = sql.replace("{NOW}", "to_char(now(), 'YYYY-MM-DD HH24:MI:SS')");
            Cow::Owned(rewrite_placeholders(&sql))
        }
    };
    // Callers supply internal SQL templates; user data is bound separately via Val.
    // Only fixed dialect syntax is interpolated here.
    AssertSqlSafe(sql.into_owned())
}

/// An open connection pool to one of the supported backends.
///
/// Cloning is cheap because the inner `sqlx` pool is reference-counted.
#[derive(Debug, Clone)]
pub enum Db {
    Sqlite(SqlitePool),
    #[cfg(feature = "postgres")]
    Postgres(PgPool),
}

impl Db {
    fn dialect(&self) -> Dialect {
        match self {
            Db::Sqlite(_) => Dialect::Sqlite,
            #[cfg(feature = "postgres")]
            Db::Postgres(_) => Dialect::Postgres,
        }
    }

    /// Run a statement, returning the number of affected rows.
    pub async fn execute(&self, sql: &str, vals: &[Val]) -> Result<u64> {
        let sql = prepare_sql(self.dialect(), sql);
        match self {
            Db::Sqlite(pool) => {
                let args = sqlite_args(vals)?;
                Ok(sqlx::query_with(sql, args)
                    .execute(pool)
                    .await?
                    .rows_affected())
            }
            #[cfg(feature = "postgres")]
            Db::Postgres(pool) => {
                let args = pg_args(vals)?;
                Ok(sqlx::query_with(sql, args)
                    .execute(pool)
                    .await?
                    .rows_affected())
            }
        }
    }

    /// Run a query and map every returned row.
    pub async fn fetch_all<T>(
        &self,
        sql: &str,
        vals: &[Val],
        map: impl Fn(&dyn DbRow) -> Result<T>,
    ) -> Result<Vec<T>> {
        let sql = prepare_sql(self.dialect(), sql);
        match self {
            Db::Sqlite(pool) => {
                let args = sqlite_args(vals)?;
                let rows = sqlx::query_with(sql, args).fetch_all(pool).await?;
                rows.iter().map(|r| map(r as &dyn DbRow)).collect()
            }
            #[cfg(feature = "postgres")]
            Db::Postgres(pool) => {
                let args = pg_args(vals)?;
                let rows = sqlx::query_with(sql, args).fetch_all(pool).await?;
                rows.iter().map(|r| map(r as &dyn DbRow)).collect()
            }
        }
    }

    /// Run a query expected to return at most one row.
    pub async fn fetch_optional<T>(
        &self,
        sql: &str,
        vals: &[Val],
        map: impl Fn(&dyn DbRow) -> Result<T>,
    ) -> Result<Option<T>> {
        let sql = prepare_sql(self.dialect(), sql);
        match self {
            Db::Sqlite(pool) => {
                let args = sqlite_args(vals)?;
                match sqlx::query_with(sql, args).fetch_optional(pool).await? {
                    Some(r) => Ok(Some(map(&r as &dyn DbRow)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "postgres")]
            Db::Postgres(pool) => {
                let args = pg_args(vals)?;
                match sqlx::query_with(sql, args).fetch_optional(pool).await? {
                    Some(r) => Ok(Some(map(&r as &dyn DbRow)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Run a query expected to return exactly one row.
    pub async fn fetch_one<T>(
        &self,
        sql: &str,
        vals: &[Val],
        map: impl Fn(&dyn DbRow) -> Result<T>,
    ) -> Result<T> {
        let sql = prepare_sql(self.dialect(), sql);
        match self {
            Db::Sqlite(pool) => {
                let args = sqlite_args(vals)?;
                let r = sqlx::query_with(sql, args).fetch_one(pool).await?;
                map(&r as &dyn DbRow)
            }
            #[cfg(feature = "postgres")]
            Db::Postgres(pool) => {
                let args = pg_args(vals)?;
                let r = sqlx::query_with(sql, args).fetch_one(pool).await?;
                map(&r as &dyn DbRow)
            }
        }
    }

    /// Begin a transaction for the few multi-statement writers that need
    /// all-or-nothing semantics (`record_install`, `remove_installed_mod`).
    pub async fn begin(&self) -> Result<DbTx<'_>> {
        Ok(match self {
            Db::Sqlite(pool) => DbTx::Sqlite(pool.begin().await?),
            #[cfg(feature = "postgres")]
            Db::Postgres(pool) => DbTx::Postgres(pool.begin().await?),
        })
    }
}

/// An in-progress transaction mirroring [`Db`]'s query surface.
pub enum DbTx<'c> {
    Sqlite(sqlx::Transaction<'c, sqlx::Sqlite>),
    #[cfg(feature = "postgres")]
    Postgres(sqlx::Transaction<'c, sqlx::Postgres>),
}

impl DbTx<'_> {
    fn dialect(&self) -> Dialect {
        match self {
            DbTx::Sqlite(_) => Dialect::Sqlite,
            #[cfg(feature = "postgres")]
            DbTx::Postgres(_) => Dialect::Postgres,
        }
    }

    pub async fn execute(&mut self, sql: &str, vals: &[Val]) -> Result<u64> {
        let sql = prepare_sql(self.dialect(), sql);
        match self {
            DbTx::Sqlite(tx) => {
                let args = sqlite_args(vals)?;
                Ok(sqlx::query_with(sql, args)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected())
            }
            #[cfg(feature = "postgres")]
            DbTx::Postgres(tx) => {
                let args = pg_args(vals)?;
                Ok(sqlx::query_with(sql, args)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected())
            }
        }
    }

    pub async fn commit(self) -> Result<()> {
        match self {
            DbTx::Sqlite(tx) => tx.commit().await?,
            #[cfg(feature = "postgres")]
            DbTx::Postgres(tx) => tx.commit().await?,
        }
        Ok(())
    }
}
