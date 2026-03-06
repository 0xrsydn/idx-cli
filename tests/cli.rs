use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("idx-cli"))
}

fn test_env_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idx-cli-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn bin_with_root(root: &Path) -> Command {
    let config_home = root.join("config");
    let cache_home = root.join("cache");
    fs::create_dir_all(&config_home).expect("create config dir");
    fs::create_dir_all(&cache_home).expect("create cache dir");

    let mut cmd = bin();
    cmd.env("XDG_CONFIG_HOME", &config_home);
    cmd.env("XDG_CACHE_HOME", &cache_home);
    cmd
}

fn test_bin(name: &str) -> Command {
    let root = test_env_dir(name);
    bin_with_root(&root)
}

#[test]
fn help_works() {
    test_bin("help").arg("--help").assert().success();
}

#[test]
fn version_prints_cargo_version() {
    test_bin("version")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn quote_table_with_mock_contains_expected_columns() {
    test_bin("quote-table")
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
    test_bin("quote-json")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "quote", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("symbol"))
        .stdout(predicate::str::contains("price"));
}

#[test]
fn history_with_mock_provider_table_contains_columns() {
    test_bin("history-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "history", "BBCA", "--period", "1mo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DATE"))
        .stdout(predicate::str::contains("OPEN"))
        .stdout(predicate::str::contains("VOLUME"));
}

#[test]
fn technical_with_mock_provider_table_contains_expected_rows() {
    test_bin("technical-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "technical", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Technical Analysis for"))
        .stdout(predicate::str::contains("RSI (14)"))
        .stdout(predicate::str::contains("Overall Signal"));
}

#[test]
fn technical_with_mock_provider_json_contains_fields() {
    test_bin("technical-json")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "technical", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\""))
        .stdout(predicate::str::contains("\"sma20\""))
        .stdout(predicate::str::contains("\"signals\""));
}

#[test]
fn growth_with_mock_provider_table_contains_expected_rows() {
    test_bin("growth-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "growth", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Growth Analysis"))
        .stdout(predicate::str::contains("Revenue Growth"))
        .stdout(predicate::str::contains("Overall"));
}

#[test]
fn growth_with_mock_provider_json_contains_fields() {
    test_bin("growth-json")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "growth", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"revenue_growth\""))
        .stdout(predicate::str::contains("\"overall_signal\""));
}

#[test]
fn valuation_with_mock_provider_table_contains_expected_rows() {
    test_bin("valuation-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "valuation", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Valuation"))
        .stdout(predicate::str::contains("P/E"))
        .stdout(predicate::str::contains("Overall"));
}

#[test]
fn risk_with_mock_provider_table_contains_expected_rows() {
    test_bin("risk-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "risk", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Risk"))
        .stdout(predicate::str::contains("Debt/Equity"))
        .stdout(predicate::str::contains("Overall"));
}

#[test]
fn fundamental_with_mock_provider_table_contains_expected_rows() {
    test_bin("fundamental-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "fundamental", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fundamental"))
        .stdout(predicate::str::contains("Overall"));
}

#[test]
fn compare_with_mock_provider_table_contains_resolved_symbol() {
    test_bin("compare-table")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "compare", "BBCA,BBRI"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BBCA.JK"));
}

#[test]
fn config_path_prints_path() {
    test_bin("config-path")
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.toml"));
}

#[test]
fn config_init_creates_file() {
    let root = test_env_dir("config-init");
    let config_home = root.join("cfg");

    bin_with_root(&root)
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

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["cache", "info"])
        .assert()
        .success();

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["cache", "clear"])
        .assert()
        .success();
}

#[test]
fn serves_stale_cache_on_provider_failure_with_warning() {
    let root = test_env_dir("stale");
    let cache_home = root.join("cache");

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success();

    bin_with_root(&root)
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
fn technical_serves_stale_cache_on_provider_failure_with_warning() {
    let root = test_env_dir("technical-stale");
    let cache_home = root.join("cache");

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "technical", "BBCA"])
        .assert()
        .success();

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "technical", "BBCA"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: network failed"));
}

#[test]
fn invalid_symbol_returns_non_zero() {
    test_bin("invalid-symbol")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "quote", "INVALID"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error:"));
}
