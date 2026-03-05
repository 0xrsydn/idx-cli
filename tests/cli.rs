use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_works() {
    Command::cargo_bin("idx-cli")
        .expect("binary exists")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn version_prints_cargo_version() {
    Command::cargo_bin("idx-cli")
        .expect("binary exists")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn quote_with_mock_provider_json() {
    Command::cargo_bin("idx-cli")
        .expect("binary exists")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "quote", "BBCA,BBRI"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BBCA.JK"))
        .stdout(predicate::str::contains("BBRI.JK"));
}

#[test]
fn history_with_mock_provider_json() {
    Command::cargo_bin("idx-cli")
        .expect("binary exists")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args([
            "-o",
            "json",
            "stocks",
            "history",
            "BBCA",
            "--period",
            "3mo",
            "--interval",
            "1d",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("2026-03-01"));
}
