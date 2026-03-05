use std::fs;

use assert_cmd::{cargo::cargo_bin, Command};
use predicates::prelude::*;

fn bin() -> Command {
    Command::new(cargo_bin("idx-cli"))
}

fn test_env_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("idx-cli-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn help_works() {
    bin()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn version_prints_cargo_version() {
    bin()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn quote_table_with_mock_contains_expected_columns() {
    bin()
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SYMBOL"))
        .stdout(predicate::str::contains("PRICE"))
        .stdout(predicate::str::contains("CHG%"));
}

#[test]
fn quote_with_mock_provider_json() {
    bin()
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "quote", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("symbol"))
        .stdout(predicate::str::contains("price"));
}

#[test]
fn history_with_mock_provider_table_contains_columns() {
    bin()
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "history", "BBCA", "--period", "1mo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DATE"))
        .stdout(predicate::str::contains("OPEN"))
        .stdout(predicate::str::contains("VOLUME"));
}

#[test]
fn config_path_prints_path() {
    bin()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.toml"));
}

#[test]
fn config_init_creates_file() {
    let root = test_env_dir("config-init");
    let config_home = root.join("cfg");

    bin()
        .env("XDG_CONFIG_HOME", &config_home)
        .args(["config", "init"])
        .assert()
        .success();

    assert!(config_home.join("idx/config.toml").exists());
}

#[test]
fn cache_info_and_clear_do_not_crash() {
    let root = test_env_dir("cache");
    let cache_home = root.join("cache");

    bin()
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["cache", "info"])
        .assert()
        .success();

    bin()
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["cache", "clear"])
        .assert()
        .success();
}

#[test]
fn serves_stale_cache_on_provider_failure_with_warning() {
    let root = test_env_dir("stale");
    let cache_home = root.join("cache");

    bin()
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success();

    bin()
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: network failed"));
}

#[test]
fn invalid_symbol_returns_non_zero() {
    bin()
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "quote", "INVALID"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error:"));
}
