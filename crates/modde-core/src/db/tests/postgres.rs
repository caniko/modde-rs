use super::*;

#[test]
fn build_pg_options_accepts_discrete_env_only_config() {
    let opts = build_pg_options(
        &DatabaseSettings::default(),
        &env_from(&[
            ("MODDE_DATABASE_HOST", "pg.example.test"),
            ("MODDE_DATABASE_PORT", "15432"),
            ("MODDE_DATABASE_NAME", "modde_env"),
            ("MODDE_DATABASE_USER", "modde_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "pg.example.test");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("modde_env"));
    assert_eq!(opts.get_username(), "modde_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_env_overrides_each_discrete_setting() {
    let settings = DatabaseSettings {
        host: Some("settings-host".to_string()),
        port: Some(5432),
        dbname: Some("settings_db".to_string()),
        user: Some("settings_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_db"),
            ("MODDE_DATABASE_USER", "env_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "env-host");
    assert_eq!(opts.get_port(), 6543);
    assert_eq!(opts.get_database(), Some("env_db"));
    assert_eq!(opts.get_username(), "env_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_uses_settings_when_env_is_empty() {
    let settings = DatabaseSettings {
        host: Some("settings-host".to_string()),
        port: Some(15432),
        dbname: Some("settings_db".to_string()),
        user: Some("settings_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(&settings, &empty_env).unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_requires_merged_database_name() {
    let err = build_pg_options(
        &DatabaseSettings {
            host: Some("settings-host".to_string()),
            ..DatabaseSettings::default()
        },
        &empty_env,
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("postgres backend selected but no database name configured")
    );
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_reads_modde_database_name_not_dbname() {
    let opts = build_pg_options(
        &DatabaseSettings::default(),
        &env_from(&[
            ("MODDE_DATABASE_DBNAME", "wrong_key"),
            ("MODDE_DATABASE_NAME", "right_key"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_database(), Some("right_key"));
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_url_takes_precedence_over_discrete_fields() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        host: Some("settings-discrete-host".to_string()),
        port: Some(5432),
        dbname: Some("settings_discrete_db".to_string()),
        user: Some("settings_discrete_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-discrete-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_discrete_db"),
            ("MODDE_DATABASE_USER", "env_discrete_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_settings_url_takes_precedence_over_env_discrete_fields() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_db"),
            ("MODDE_DATABASE_USER", "env_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_env_url_takes_precedence_over_settings_url() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[(
            "MODDE_DATABASE_URL",
            "postgres://env_user@env-host:6543/env_db",
        )]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "env-host");
    assert_eq!(opts.get_port(), 6543);
    assert_eq!(opts.get_database(), Some("env_db"));
    assert_eq!(opts.get_username(), "env_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_rejects_bad_env_port() {
    let err = build_pg_options(
        &DatabaseSettings {
            dbname: Some("settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        &env_from(&[("MODDE_DATABASE_PORT", "not-a-port")]),
    )
    .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("invalid MODDE_DATABASE_PORT value 'not-a-port'"));
    assert!(!message.contains("settings_db"));
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_rejects_out_of_range_env_port() {
    let err = build_pg_options(
        &DatabaseSettings {
            dbname: Some("settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        &env_from(&[("MODDE_DATABASE_PORT", "70000")]),
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid MODDE_DATABASE_PORT value '70000'")
    );
}

#[cfg(feature = "postgres")]
#[test]
fn read_pg_password_file_trims_trailing_newline() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pg-password");
    std::fs::write(&path, "secret-password\n").unwrap();

    let password = read_pg_password_file(&path).unwrap();

    assert_eq!(password, "secret-password");
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[serial]
async fn open_with_settings_honors_env_backend_override() {
    let tmp = tempfile::tempdir().unwrap();
    let _backend = EnvRestore::set("MODDE_DATABASE_BACKEND", "sqlite");
    let _url = EnvRestore::set(
        "MODDE_DATABASE_URL",
        "postgres://env_user@127.0.0.1:1/env_db",
    );
    let data_home = tmp.path().join("data");
    let config_home = tmp.path().join("config");
    let _xdg_data = EnvRestore::set("XDG_DATA_HOME", data_home.to_str().unwrap());
    let _xdg_config = EnvRestore::set("XDG_CONFIG_HOME", config_home.to_str().unwrap());

    let settings = AppSettings {
        database: DatabaseSettings {
            backend: DbBackend::Postgres,
            url: Some("postgres://settings_user@127.0.0.1:1/settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        ..AppSettings::default()
    };

    let db = ModdeDb::open_with_settings(&settings).await.unwrap();

    db.ping().await.unwrap();
    assert!(data_home.join("modde/modde.db").exists());
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_defaults_user_to_the_os_account() {
    // A socket URL without a user relies on sqlx's OS-user default for peer
    // auth. whoami's stub backend would report "anonymous" instead.
    let opts = build_pg_options(
        &DatabaseSettings::default(),
        &env_from(&[(
            "MODDE_DATABASE_URL",
            "postgres:///modde?host=/run/postgresql",
        )]),
    )
    .unwrap();

    assert_ne!(opts.get_username(), "anonymous");
}
