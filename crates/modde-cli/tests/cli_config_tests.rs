mod common;

use std::path::Path;

use common::Fixture;
use modde_core::settings::{AppSettings, DbBackend};

fn settings_path(fx: &Fixture) -> std::path::PathBuf {
    fx.home().join(".config/modde/settings.toml")
}

fn read_settings(fx: &Fixture) -> AppSettings {
    let path = settings_path(fx);
    assert!(
        path.starts_with(fx.root()),
        "settings path must stay inside fixture root: {}",
        path.display()
    );
    let content = std::fs::read_to_string(&path).expect("read settings.toml");
    toml::from_str(&content).expect("parse settings.toml")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: std::process::Output) -> std::process::Output {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    output
}

fn assert_failure(output: std::process::Output) -> std::process::Output {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    output
}

fn set_discrete_postgres(fx: &Fixture) {
    assert_success(
        fx.cmd()
            .args([
                "config",
                "set-database",
                "--backend",
                "postgres",
                "--name",
                "modde",
                "--host",
                "db.example.test",
                "--port",
                "5432",
                "--user",
                "modde_user",
            ])
            .output()
            .expect("spawn config set-database"),
    );
}

#[test]
fn set_database_writes_expected_settings_toml_without_password() {
    let fx = Fixture::new();

    set_discrete_postgres(&fx);

    let settings = read_settings(&fx);
    assert_eq!(settings.database.backend, DbBackend::Postgres);
    assert_eq!(settings.database.url, None);
    assert_eq!(settings.database.host.as_deref(), Some("db.example.test"));
    assert_eq!(settings.database.port, Some(5432));
    assert_eq!(settings.database.dbname.as_deref(), Some("modde"));
    assert_eq!(settings.database.user.as_deref(), Some("modde_user"));
    assert_eq!(settings.database.password_file, None);

    let content = std::fs::read_to_string(settings_path(&fx)).expect("read settings.toml");
    assert!(content.contains("[database]"));
    assert!(!content.contains("password"));
}

#[test]
#[cfg(feature = "postgres")]
fn show_reports_settings_and_discrete_env_overrides_with_sources() {
    let fx = Fixture::new();
    set_discrete_postgres(&fx);

    let stored = assert_success(
        fx.cmd()
            .args(["config", "show"])
            .output()
            .expect("spawn config show"),
    );
    let stored_stdout = stdout(&stored);
    assert!(
        stored_stdout.contains("database backend: postgres (from settings.toml)"),
        "unexpected show output:\n{stored_stdout}"
    );
    assert!(
        stored_stdout.contains("host: db.example.test (from settings.toml)"),
        "unexpected show output:\n{stored_stdout}"
    );
    assert!(
        stored_stdout.contains("port: 5432 (from settings.toml)"),
        "unexpected show output:\n{stored_stdout}"
    );
    assert!(
        stored_stdout.contains("dbname: modde (from settings.toml)"),
        "unexpected show output:\n{stored_stdout}"
    );
    assert!(
        stored_stdout.contains("user: modde_user (from settings.toml)"),
        "unexpected show output:\n{stored_stdout}"
    );

    let env = assert_success(
        fx.cmd()
            .env("MODDE_DATABASE_HOST", "env-db.example.test")
            .env("MODDE_DATABASE_PORT", "15432")
            .env("MODDE_DATABASE_NAME", "env_modde")
            .env("MODDE_DATABASE_USER", "env_user")
            .args(["config", "show"])
            .output()
            .expect("spawn config show with env"),
    );
    let env_stdout = stdout(&env);
    assert!(
        env_stdout.contains("host: env-db.example.test (from MODDE_DATABASE_HOST)"),
        "unexpected show output:\n{env_stdout}"
    );
    assert!(
        env_stdout.contains("port: 15432 (from MODDE_DATABASE_PORT)"),
        "unexpected show output:\n{env_stdout}"
    );
    assert!(
        env_stdout.contains("dbname: env_modde (from MODDE_DATABASE_NAME)"),
        "unexpected show output:\n{env_stdout}"
    );
    assert!(
        env_stdout.contains("user: env_user (from MODDE_DATABASE_USER)"),
        "unexpected show output:\n{env_stdout}"
    );
}

#[test]
#[cfg(not(feature = "postgres"))]
fn show_rejects_disabled_postgres_backend_without_connecting() {
    let fx = Fixture::new();
    set_discrete_postgres(&fx);
    let output = assert_failure(
        fx.cmd()
            .args(["config", "show"])
            .output()
            .expect("spawn config show"),
    );
    assert!(stdout(&output).contains("database backend: postgres (from settings.toml)"));
    assert!(stderr(&output).contains("PostgreSQL support requires the postgres feature"));
}

#[test]
fn clear_reset_and_empty_string_normalization_remove_postgres_fields() {
    let fx = Fixture::new();
    set_discrete_postgres(&fx);

    assert_success(
        fx.cmd()
            .args([
                "config",
                "set-database",
                "--backend",
                "postgres",
                "--name",
                "modde",
                "--clear",
                "host",
            ])
            .output()
            .expect("spawn config clear host"),
    );
    let settings = read_settings(&fx);
    assert_eq!(settings.database.host, None);
    assert_eq!(settings.database.dbname.as_deref(), Some("modde"));

    assert_success(
        fx.cmd()
            .args([
                "config",
                "set-database",
                "--backend",
                "postgres",
                "--name",
                "modde",
                "--url",
                "",
            ])
            .output()
            .expect("spawn config clear url"),
    );
    let settings = read_settings(&fx);
    assert_eq!(settings.database.url, None);

    assert_success(
        fx.cmd()
            .args(["config", "set-database", "--backend", "sqlite"])
            .output()
            .expect("spawn config set sqlite"),
    );
    let settings = read_settings(&fx);
    assert_eq!(settings.database.backend, DbBackend::Sqlite);
    assert_eq!(settings.database.url, None);
    assert_eq!(settings.database.host, None);
    assert_eq!(settings.database.port, None);
    assert_eq!(settings.database.dbname, None);
    assert_eq!(settings.database.user, None);
    assert_eq!(settings.database.password_file, None);

    set_discrete_postgres(&fx);
    assert_success(
        fx.cmd()
            .args(["config", "reset-database"])
            .output()
            .expect("spawn config reset-database"),
    );
    let settings = read_settings(&fx);
    assert_eq!(settings.database.backend, DbBackend::Sqlite);
    assert_eq!(settings.database.url, None);
    assert_eq!(settings.database.host, None);
    assert_eq!(settings.database.port, None);
    assert_eq!(settings.database.dbname, None);
    assert_eq!(settings.database.user, None);
    assert_eq!(settings.database.password_file, None);
}

#[test]
fn set_database_rejects_invalid_postgres_shapes() {
    let fx = Fixture::new();

    let missing_name = assert_failure(
        fx.cmd()
            .args(["config", "set-database", "--backend", "postgres"])
            .output()
            .expect("spawn invalid config set-database"),
    );
    let missing_name_stderr = stderr(&missing_name);
    assert!(
        missing_name_stderr.contains("requires either --url, or at least --name"),
        "unexpected stderr:\n{missing_name_stderr}"
    );

    let mixed = assert_failure(
        fx.cmd()
            .args([
                "config",
                "set-database",
                "--backend",
                "postgres",
                "--url",
                "postgres://modde@db.example.test/modde",
                "--name",
                "modde",
            ])
            .output()
            .expect("spawn invalid mixed config set-database"),
    );
    let mixed_stderr = stderr(&mixed);
    assert!(
        mixed_stderr.contains("set --url OR the discrete --host/--port/--name/--user fields"),
        "unexpected stderr:\n{mixed_stderr}"
    );
}

#[test]
fn config_test_sqlite_reports_ok_under_isolated_data_dir() {
    let fx = Fixture::new();

    let output = assert_success(
        fx.cmd()
            .args(["config", "test"])
            .output()
            .expect("spawn config test"),
    );
    let output = stdout(&output);
    assert!(
        output.contains("database backend: sqlite"),
        "unexpected output:\n{output}"
    );
    assert!(output.contains("OK"), "unexpected output:\n{output}");
    assert!(
        output.contains(&format!(
            "summary: path={}",
            fx.data_dir().join("modde.db").display()
        )),
        "unexpected output:\n{output}"
    );
}

#[test]
fn database_settings_serde_keeps_none_fields_absent_and_reads_legacy_partials() {
    let partial = r#"
backend = "postgres"
dbname = "legacy_modde"
"#;
    let parsed: modde_core::settings::DatabaseSettings =
        toml::from_str(partial).expect("parse legacy partial database settings");
    assert_eq!(parsed.backend, DbBackend::Postgres);
    assert_eq!(parsed.dbname.as_deref(), Some("legacy_modde"));
    assert_eq!(parsed.url, None);
    assert_eq!(parsed.host, None);
    assert_eq!(parsed.port, None);
    assert_eq!(parsed.user, None);
    assert_eq!(parsed.password_file, None);

    let serialized = toml::to_string(&parsed).expect("serialize database settings");
    assert!(serialized.contains("backend = \"postgres\""));
    assert!(serialized.contains("dbname = \"legacy_modde\""));
    assert!(!serialized.contains("url ="));
    assert!(!serialized.contains("host ="));
    assert!(!serialized.contains("port ="));
    assert!(!serialized.contains("user ="));
    assert!(!serialized.contains("password_file ="));
}

#[test]
fn optional_postgres_doctor_redacts_password_when_modde_test_pg_url_is_set() {
    let Ok(pg_url) = std::env::var("MODDE_TEST_PG_URL") else {
        return;
    };
    let fx = Fixture::new();

    let output = assert_success(
        fx.cmd()
            .env("MODDE_DATABASE_BACKEND", "postgres")
            .env("MODDE_DATABASE_URL", &pg_url)
            .args(["config", "test"])
            .output()
            .expect("spawn config test against postgres"),
    );
    let combined = format!("{}\n{}", stdout(&output), stderr(&output));
    assert!(
        combined.contains("database backend: postgres"),
        "unexpected output:\n{combined}"
    );
    assert!(combined.contains("OK"), "unexpected output:\n{combined}");

    if let Ok(url) = url::Url::parse(&pg_url)
        && let Some(password) = url.password()
    {
        assert!(
            !password.is_empty() && !combined.contains(password),
            "doctor output leaked the URL password:\n{combined}"
        );
    }
}

#[test]
fn fixture_config_path_is_isolated_from_real_home() {
    let fx = Fixture::new();
    set_discrete_postgres(&fx);
    let path = settings_path(&fx);
    assert!(Path::new(&path).starts_with(fx.root()));
    assert!(path.exists());
}
