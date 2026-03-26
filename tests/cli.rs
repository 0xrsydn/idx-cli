use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    let current = std::env::current_exe().expect("current test executable path");
    let debug_dir = current
        .parent()
        .and_then(|path| path.parent())
        .expect("target debug directory");
    let exe = debug_dir.join(format!("idx{}", std::env::consts::EXE_SUFFIX));
    Command::new(exe)
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
fn msn_history_auto_falls_back_to_yahoo() {
    test_bin("msn-history-auto-fallback")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "history", "BBCA", "--period", "3mo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DATE"));
}

#[test]
fn msn_technical_auto_falls_back_to_yahoo() {
    test_bin("msn-technical-auto-fallback")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "technical", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\""));
}

#[test]
fn explicit_msn_history_provider_returns_unsupported() {
    test_bin("msn-history-explicit-unsupported")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args([
            "stocks",
            "history",
            "BBCA",
            "--period",
            "3mo",
            "--history-provider",
            "msn",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "MSN does not provide price history",
        ));
}

#[test]
fn profile_requires_msn_provider_in_json_mode() {
    test_bin("profile-json-provider-gate")
        .env("IDX_PROVIDER", "yahoo")
        .args(["-o", "json", "stocks", "profile", "BBCA"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"error\": true"))
        .stderr(predicate::str::contains("requires --provider msn"));
}

#[test]
fn msn_profile_with_mock_fixture_table_contains_expected_fields() {
    test_bin("msn-profile-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "profile", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Symbol"))
        .stdout(predicate::str::contains("Bank Central Asia Tbk PT"));
}

#[test]
fn msn_financials_with_mock_fixture_table_contains_sections() {
    test_bin("msn-financials-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "financials", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Income Statement"))
        .stdout(predicate::str::contains("netIncome"))
        .stdout(predicate::str::contains("Cash Flow"));
}

#[test]
fn msn_earnings_with_mock_fixture_json_contains_forecast_and_history() {
    test_bin("msn-earnings-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "earnings", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"forecast\""))
        .stdout(predicate::str::contains("\"history\""))
        .stdout(predicate::str::contains("Q12026"));
}

#[test]
fn msn_sentiment_with_mock_fixture_table_contains_ranges() {
    test_bin("msn-sentiment-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "sentiment", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RANGE"))
        .stdout(predicate::str::contains("1D"))
        .stdout(predicate::str::contains("BULLISH"));
}

#[test]
fn msn_insights_with_mock_fixture_table_contains_highlights_and_risks() {
    test_bin("msn-insights-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "insights", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Highlights:"))
        .stdout(predicate::str::contains(
            "Shares trade near historical averages.",
        ))
        .stdout(predicate::str::contains("Risks:"));
}

#[test]
fn msn_news_with_mock_fixture_table_contains_provider_and_title() {
    test_bin("msn-news-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "news", "BBCA", "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BCA reports steady growth"))
        .stdout(predicate::str::contains("Contoso News"));
}

#[test]
fn msn_screen_with_mock_fixture_table_contains_quotes() {
    test_bin("msn-screen-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args([
            "stocks",
            "screen",
            "--filter",
            "top-performers",
            "--limit",
            "10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("SYMBOL"))
        .stdout(predicate::str::contains("BBCA.JK"))
        .stdout(predicate::str::contains("BBRI.JK"));
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
    let raw = fs::read_to_string(config_home.join("idx/config.toml")).expect("read config");
    assert!(raw.contains("provider = \"msn\""));
    assert!(raw.contains("history_provider = \"auto\""));
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
fn config_set_and_get_provider_round_trip() {
    let root = test_env_dir("config-provider");

    bin_with_root(&root)
        .args(["config", "set", "general.provider", "msn"])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["config", "get", "general.provider"])
        .assert()
        .success()
        .stdout(predicate::str::contains("msn"));
}

#[test]
fn config_set_and_get_ownership_db_path_round_trip() {
    let root = test_env_dir("config-ownership-db-path");

    bin_with_root(&root)
        .args(["config", "set", "ownership.db_path", "/tmp/ownership.db"])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["config", "get", "ownership.db_path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("/tmp/ownership.db"));
}

#[test]
fn config_set_mixed_case_provider_does_not_break_future_loads() {
    let root = test_env_dir("config-mixed-case-provider");

    bin_with_root(&root)
        .args(["config", "init"])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["config", "set", "general.provider", "Msn"])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["version"])
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn ownership_import_fetch_bing_reports_unsupported() {
    test_bin("ownership-fetch-bing-unsupported")
        .args(["ownership", "import", "--fetch-bing", "BBCA"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--fetch-bing import is not implemented yet",
        ));
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
fn cache_namespace_isolated_by_provider() {
    let root = test_env_dir("provider-cache");
    let cache_home = root.join("cache");

    // Populate Yahoo cache
    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success();

    // MSN mock succeeds independently (uses MSN fixtures, not Yahoo's cache)
    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_QUOTE_TTL", "0")
        .args(["stocks", "quote", "BBCA"])
        .assert()
        .success();

    // MSN with mock error fails — Yahoo's cached data is NOT reused for MSN
    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
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
    test_bin("invalid-symbol")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "quote", "INVALID"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error:"));
}

#[test]
fn invalid_provider_env_returns_non_zero() {
    test_bin("invalid-provider")
        .env("IDX_PROVIDER", "bogus")
        .args(["version"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid provider"));
}
