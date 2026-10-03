use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;

use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

fn bin() -> Command {
    let exe = resolve_idx_binary_path();
    Command::new(exe)
}

fn resolve_idx_binary_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_idx").map(PathBuf::from)
        && candidate_is_idx_binary(&path)
    {
        return path;
    }

    let target_debug = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join(format!("idx{}", std::env::consts::EXE_SUFFIX));
    if candidate_is_idx_binary(&target_debug) {
        return target_debug;
    }

    let current = std::env::current_exe().expect("current test executable path");
    let deps_dir = current.parent().expect("deps directory");
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let mut candidates: Vec<PathBuf> = fs::read_dir(deps_dir)
        .expect("read deps dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| is_executable(path))
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return false;
            };
            name.starts_with("idx-")
                && !name.ends_with(".d")
                && (exe_suffix.is_empty() || name.ends_with(exe_suffix))
        })
        .collect();

    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .expect("modified time")
    });
    candidates.reverse();

    for candidate in candidates {
        if candidate_is_idx_binary(&candidate) {
            return candidate;
        }
    }

    panic!("unable to resolve idx app binary in {}", deps_dir.display());
}

fn candidate_is_idx_binary(path: &Path) -> bool {
    if !path.is_file() || !is_executable(path) {
        return false;
    }

    let Ok(output) = std::process::Command::new(path).arg("version").output() else {
        return false;
    };

    output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == env!("CARGO_PKG_VERSION")
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("exe"))
        .unwrap_or(true)
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
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd.env("XDG_CONFIG_HOME", &config_home);
    cmd.env("XDG_CACHE_HOME", &cache_home);
    cmd
}

fn test_bin(name: &str) -> Command {
    let root = test_env_dir(name);
    bin_with_root(&root)
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_success_stdout(cmd: &mut Command) -> String {
    let output = cmd.output().expect("run command");
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

fn run_success_json(cmd: &mut Command) -> Value {
    serde_json::from_str(&run_success_stdout(cmd)).expect("stdout contains only JSON")
}

fn run_error_json(cmd: &mut Command, code: &str) {
    let output = cmd.output().expect("run failing command");
    assert!(!output.status.success(), "unexpected success: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "error polluted stdout: {output:?}"
    );
    let error: Value = serde_json::from_slice(&output.stderr).expect("stderr contains JSON error");
    assert_eq!(error["error"], true);
    assert_eq!(error["code"], code);
}

fn spawn_single_response_server(content_type: &str, body: impl Into<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
    let addr = listener.local_addr().expect("local addr");
    let content_type = content_type.to_string();
    let body = body.into();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept test connection");
        let mut buf = [0u8; 2048];
        let _ = stream.read(&mut buf);

        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .expect("write response headers");
        stream.write_all(&body).expect("write response body");
    });

    format!("http://{addr}")
}

fn fake_pdf_bytes() -> Vec<u8> {
    b"%PDF-1.7\n% idx-cli test fixture\n".to_vec()
}

fn install_fake_mutool(root: &Path, xml: &str) -> PathBuf {
    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("create fake bin dir");
    let mutool_path = bin_dir.join("mutool");
    let script = format!(
        "#!/bin/sh\n\
if [ \"$1\" = \"--help\" ]; then\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"convert\" ]; then\n\
  cat <<'__IDX_XML__'\n\
{xml}\n\
__IDX_XML__\n\
  exit 0\n\
fi\n\
echo \"unexpected mutool args: $@\" >&2\n\
exit 1\n"
    );
    fs::write(&mutool_path, script).expect("write fake mutool");
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&mutool_path)
            .expect("fake mutool metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mutool_path, perms).expect("set fake mutool perms");
    }
    bin_dir
}

fn install_fake_mutool_routes(root: &Path, routes: &[(&str, &str)]) -> PathBuf {
    let bin_dir = root.join("fake-bin-routes");
    fs::create_dir_all(&bin_dir).expect("create fake route bin dir");
    let mutool_path = bin_dir.join("mutool");

    let mut script = String::from(
        "#!/bin/sh\n\
if [ \"$1\" = \"--help\" ]; then\n\
  exit 0\n\
fi\n\
if [ \"$1\" != \"convert\" ]; then\n\
  echo \"unexpected mutool args: $@\" >&2\n\
  exit 1\n\
fi\n\
last=\"\"\n\
for arg in \"$@\"; do\n\
  last=\"$arg\"\n\
done\n\
case \"$last\" in\n",
    );

    for (index, (suffix, xml)) in routes.iter().enumerate() {
        script.push_str(&format!(
            "  *{suffix})\n\
    cat <<'__IDX_XML_{index}__'\n\
{xml}\n\
__IDX_XML_{index}__\n\
    exit 0\n\
    ;;\n"
        ));
    }

    script.push_str(
        "  *)\n\
    echo \"unexpected mutool target: $last\" >&2\n\
    exit 1\n\
    ;;\n\
esac\n",
    );

    fs::write(&mutool_path, script).expect("write fake routed mutool");
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&mutool_path)
            .expect("fake routed mutool metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mutool_path, perms).expect("set fake routed mutool perms");
    }

    bin_dir
}

fn prepend_path(dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(current) if !current.is_empty() => format!("{}:{current}", dir.display()),
        _ => dir.display().to_string(),
    }
}

fn pdf_url(base: &str, name: &str) -> String {
    format!("{base}/{name}.pdf")
}

fn write_zip_with_text(zip_path: &Path, entry_name: &str, text: &str) {
    let file = fs::File::create(zip_path).expect("create zip fixture");
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(entry_name, SimpleFileOptions::default())
        .expect("start zip file");
    writer
        .write_all(text.as_bytes())
        .expect("write zip fixture text");
    writer.finish().expect("finish zip fixture");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn snapshot_metadata(db_path: &Path) -> (usize, String, String, usize, usize) {
    let conn = Connection::open(db_path).expect("open snapshot sqlite");
    let release_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ownership_releases", [], |row| {
            row.get(0)
        })
        .expect("snapshot release count");
    let (latest_as_of_date, latest_release_sha256, latest_row_count): (String, String, i64) = conn
        .query_row(
            "SELECT as_of_date, sha256, row_count
             FROM ownership_releases
             ORDER BY as_of_date DESC, imported_at DESC
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("snapshot latest release metadata");
    let ticker_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tickers", [], |row| row.get(0))
        .expect("snapshot ticker count");

    (
        usize::try_from(release_count).expect("release_count fits usize"),
        latest_as_of_date,
        latest_release_sha256,
        usize::try_from(latest_row_count).expect("row_count fits usize"),
        usize::try_from(ticker_count).expect("ticker_count fits usize"),
    )
}

fn write_snapshot_manifest(path: &Path, db_path: &Path, checksum_override: Option<&str>) {
    let bytes = fs::read(db_path).expect("read snapshot sqlite");
    let (release_count, latest_as_of_date, latest_release_sha256, latest_row_count, ticker_count) =
        snapshot_metadata(db_path);
    let manifest = serde_json::json!({
        "schema_version": 1,
        "generated_at": "2026-03-31T12:00:00Z",
        "snapshot": {
            "kind": "sqlite",
            "compression": "none",
            "version": latest_as_of_date,
            "download_url": db_path.to_str().expect("snapshot db path"),
            "sqlite_sha256": checksum_override.unwrap_or(&sha256_hex(&bytes)),
            "size_bytes": bytes.len(),
            "release_count": release_count,
            "latest_as_of_date": latest_as_of_date,
            "latest_release_sha256": latest_release_sha256,
            "latest_row_count": latest_row_count,
            "ticker_count": ticker_count
        }
    });

    fs::write(
        path,
        serde_json::to_string_pretty(&manifest).expect("serialize snapshot manifest"),
    )
    .expect("write snapshot manifest");
}

fn prepare_snapshot_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let source_db = root.join("published-ownership.db");
    let manifest_path = root.join("ownership-snapshot-manifest.json");
    let first_pdf = root.join("release-2026-01-31.pdf");
    let second_pdf = root.join("release-2026-02-27.pdf");
    fs::write(&first_pdf, b"%PDF-1.7\n% snapshot fixture jan\n").expect("write jan pdf");
    fs::write(&second_pdf, b"%PDF-1.7\n% snapshot fixture feb\n").expect("write feb pdf");

    let fake_mutool_dir = install_fake_mutool_routes(
        root,
        &[
            (
                "release-2026-01-31.pdf",
                include_str!("fixtures/ksei_above1_stext_excerpt_prev.xml"),
            ),
            (
                "release-2026-02-27.pdf",
                include_str!("fixtures/ksei_above1_stext_excerpt.xml"),
            ),
        ],
    );

    bin_with_root(root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            source_db.to_str().unwrap(),
        ])
        .assert()
        .success();

    for file in [&first_pdf, &second_pdf] {
        bin_with_root(root)
            .env("PATH", prepend_path(&fake_mutool_dir))
            .args(["ownership", "import", "--file", file.to_str().unwrap()])
            .assert()
            .success();
    }

    write_snapshot_manifest(&manifest_path, &source_db, None);
    (source_db, manifest_path)
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
    let quotes = run_success_json(
        test_bin("quote-json")
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "quote", "BBCA"]),
    );
    assert_eq!(quotes.as_array().unwrap().len(), 1);
    let quote = &quotes[0];
    assert_eq!(quote["symbol"], "BBCA.JK");
    assert_eq!(quote["price"].as_i64(), Some(9875));
    assert_eq!(quote["change"].as_i64(), Some(117));
    assert_eq!(quote["volume"].as_u64(), Some(12_300_000));
    assert_eq!(quote["prev_close"].as_i64(), Some(9758));
    assert_eq!(quote["week52_high"].as_i64(), Some(10250));
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
fn technical_json_matches_history_and_auto_provider_fallback() {
    let yahoo = run_success_json(
        test_bin("technical-yahoo-json")
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "technical", "BBCA"]),
    );
    let msn_auto = run_success_json(
        test_bin("technical-msn-auto-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "technical", "BBCA"]),
    );
    assert_eq!(yahoo, msn_auto);
    let history = run_success_json(
        test_bin("history-json")
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "history", "BBCA", "--period", "3mo"]),
    );
    let rows = history.as_array().unwrap();
    let latest = rows.last().unwrap();
    assert_eq!(yahoo["symbol"], "BBCA.JK");
    assert_eq!(yahoo["current_price"], latest["close"]);
    assert_eq!(yahoo["as_of"], latest["date"]);
    assert_eq!(
        history,
        serde_json::json!([
            {"date": "2024-03-01", "open": 9800, "high": 9900, "low": 9750, "close": 9875, "volume": 12300000},
            {"date": "2024-03-02", "open": 9850, "high": 9920, "low": 9800, "close": 9880, "volume": 11000000},
            {"date": "2024-03-03", "open": 9860, "high": 9950, "low": 9820, "close": 9925, "volume": 14000000}
        ])
    );
    assert_eq!(yahoo.get("sma20"), Some(&Value::Null));
    assert_eq!(yahoo.get("sma200"), Some(&Value::Null));
    let msn_history = run_success_json(
        test_bin("history-msn-auto-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "history", "BBCA", "--period", "3mo"]),
    );
    assert_eq!(history, msn_history);
}

#[test]
fn explicit_msn_history_provider_uses_msn_chart_fixture() {
    test_bin("msn-history-explicit")
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
        .success()
        .stdout(predicate::str::contains("History for BBCA.JK"))
        .stdout(predicate::str::contains("8,000"));
}

#[test]
fn msn_profile_json_preserves_company_fields() {
    let profile = run_success_json(
        test_bin("msn-profile-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "profile", "BBCA"]),
    );
    assert_eq!(profile["symbol"], "BBCA");
    assert_eq!(profile["long_name"], "PT Bank Central Asia Tbk");
    assert_eq!(profile["industry"], "Banking Services");
    assert_eq!(profile["country"], "Indonesia");
    assert_eq!(profile["website"], "https://www.bca.co.id/");
}

#[test]
fn msn_financials_with_statement_filter_table_only_shows_requested_section() {
    test_bin("msn-financials-statement-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "financials", "BBCA", "--statement", "cashflow"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cash Flow"))
        .stdout(predicate::str::contains("Operating Cash Flow"))
        .stdout(predicate::str::contains("Income Statement").not())
        .stdout(predicate::str::contains("Balance Sheet").not());
}

#[test]
fn msn_financials_with_statement_filter_json_keeps_context_and_nulls_filtered_sections() {
    let full = run_success_json(
        test_bin("msn-financials-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "financials", "BBCA"]),
    );
    assert_eq!(full["instrument"]["symbol"], "BBCA.JK");
    assert_eq!(full["income_statement"]["currency"], "IDR");
    assert_eq!(full["income_statement"]["report_date"], "2025-12-31");
    assert_eq!(
        full["income_statement"]["values"]["netIncome"].as_f64(),
        Some(400_000_000.0)
    );
    assert_eq!(
        full["cash_flow"]["values"]["operatingCashFlow"].as_f64(),
        Some(600_000_000.0)
    );
    for (filter, omitted) in [
        ("income,balance", vec!["cash_flow"]),
        ("cashflow", vec!["income_statement", "balance_sheet"]),
    ] {
        let filtered = run_success_json(
            test_bin(&format!("financial-filter-{filter}"))
                .env("IDX_PROVIDER", "msn")
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args([
                    "-o",
                    "json",
                    "stocks",
                    "financials",
                    "BBCA",
                    "--statement",
                    filter,
                ]),
        );
        let mut expected = full.clone();
        for section in omitted {
            expected[section] = Value::Null;
        }
        assert_eq!(filtered, expected);
    }
}

#[test]
fn msn_earnings_with_filters_table_limits_scope_and_period() {
    test_bin("msn-earnings-filter-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "earnings", "BBCA", "--history", "--quarterly"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Earnings History"))
        .stdout(predicate::str::contains("Q4 2025"))
        .stdout(predicate::str::contains("FY2025").not())
        .stdout(predicate::str::contains("Earnings Forecast").not())
        .stdout(predicate::str::contains("Q1 2026").not());
}

#[test]
fn msn_earnings_json_filters_preserve_values() {
    let full = run_success_json(
        test_bin("msn-earnings-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "earnings", "BBCA"]),
    );
    assert_eq!(full["symbol"], "BBCA.JK");
    assert_eq!(full["eps_last_year"].as_f64(), Some(1200.0));
    assert_eq!(full["forecast"].as_array().unwrap().len(), 2);
    assert_eq!(full["history"].as_array().unwrap().len(), 2);
    for (scope, period, key, period_type, field, value) in [
        (
            "--forecast",
            "--annual",
            "forecast",
            "2026",
            "eps_forecast",
            1300.0,
        ),
        (
            "--history",
            "--quarterly",
            "history",
            "Q42025",
            "eps_actual",
            320.0,
        ),
    ] {
        let filtered = run_success_json(
            test_bin(&format!("earnings-{scope}-{period}"))
                .env("IDX_PROVIDER", "msn")
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args(["-o", "json", "stocks", "earnings", "BBCA", scope, period]),
        );
        let mut expected = full.clone();
        expected["forecast"] = serde_json::json!([]);
        expected["history"] = serde_json::json!([]);
        let row = full[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["period_type"] == period_type)
            .unwrap();
        assert_eq!(row[field].as_f64(), Some(value));
        expected[key] = serde_json::json!([row]);
        assert_eq!(filtered, expected);
    }
}

#[test]
fn msn_sentiment_json_preserves_counts() {
    let data = run_success_json(
        test_bin("msn-sentiment-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "sentiment", "BBCA"]),
    );
    assert_eq!(data["symbol"], "BBCA.JK");
    let day = data["statistics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["time_range"] == "1D")
        .unwrap();
    assert_eq!(day["bullish"].as_i64(), Some(10));
    assert_eq!(day["neutral"].as_i64(), Some(3));
}

#[test]
fn msn_insights_and_news_json_preserve_fixture_content() {
    let insights = run_success_json(
        test_bin("msn-insights-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "insights", "BBCA"]),
    );
    assert_eq!(insights["symbol"], "BBCA.JK");
    assert_eq!(insights["last_updated"], "2026-03-26T04:14:57.9197955Z");
    assert!(
        insights["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|risk| risk == "Quarterly Revenue YoY Growth: Revenue grew worse than peers")
    );
    let news = run_success_json(
        test_bin("msn-news-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "news", "BBCA", "--limit", "1"]),
    );
    assert_eq!(news.as_array().unwrap().len(), 1);
    assert_eq!(news[0]["id"], "news-1");
    assert_eq!(news[0]["symbol"], "BBCA.JK");
    assert_eq!(news[0]["title"], "BCA reports steady growth");
    assert_eq!(news[0]["provider"], "Contoso News");
    assert_eq!(news[0]["published_at"], "2026-03-20T10:00:00Z");
}

#[test]
fn analysis_json_preserves_metrics_across_reports_and_compare() {
    let fundamental = run_success_json(
        test_bin("fundamental-json")
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "fundamental", "BBCA"]),
    );
    assert_eq!(fundamental["symbol"], "BBCA.JK");
    assert_eq!(
        fundamental["growth"]["revenue_growth"].as_f64(),
        Some(0.118)
    );
    assert_eq!(fundamental["valuation"]["pe_trailing"].as_f64(), Some(25.4));
    assert_eq!(fundamental["risk"]["current_ratio"].as_f64(), Some(1.21));
    for command in ["growth", "valuation", "risk"] {
        let report = run_success_json(
            test_bin(&format!("analysis-{command}"))
                .env("IDX_PROVIDER", "yahoo")
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args(["-o", "json", "stocks", command, "BBCA"]),
        );
        assert_eq!(report, fundamental[command]);
    }
    let comparison = run_success_json(
        test_bin("compare-json")
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "compare", "BBCA,BBRI"]),
    );
    assert_eq!(comparison.as_array().unwrap().len(), 2);
    assert_eq!(comparison[0], fundamental);
    assert_eq!(comparison[1]["symbol"], "BBRI.JK");
}

#[test]
fn compare_partial_offline_success_preserves_data_and_diagnostics() {
    let root = test_env_dir("compare-partial-offline");
    let expected = run_success_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "fundamental", "BBCA"]),
    );
    for quiet in [false, true] {
        let mut cmd = bin_with_root(&root);
        cmd.env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .env("IDX_MOCK_ERROR", "1")
            .args(["-o", "json", "--offline", "stocks", "compare", "BBRI,BBCA"]);
        if quiet {
            cmd.arg("--quiet");
        }
        let output = cmd.output().expect("partial comparison");
        assert!(output.status.success(), "{output:?}");
        let actual: Value = serde_json::from_slice(&output.stdout).expect("clean JSON stdout");
        assert_eq!(actual.as_array().unwrap(), std::slice::from_ref(&expected));
        assert_eq!(output.stderr.is_empty(), quiet);
    }
}

#[test]
fn nonfinite_msn_fundamentals_are_null_in_analysis_json() {
    for fixture in [
        "msn_keyratios_infinity.json",
        "msn_keyratios_negative_infinity.json",
    ] {
        for command in ["growth", "valuation", "risk", "fundamental", "compare"] {
            let report = run_success_json(
                test_bin(&format!("{fixture}-{command}"))
                    .env("IDX_PROVIDER", "msn")
                    .env("IDX_USE_MOCK_PROVIDER", "1")
                    .env("IDX_MOCK_MSN_KEYRATIOS_FIXTURE", fixture_path(fixture))
                    .args([
                        "-o",
                        "json",
                        "stocks",
                        command,
                        if command == "compare" {
                            "BBCA,BBRI"
                        } else {
                            "BBCA"
                        },
                    ]),
            );
            match command {
                "valuation" => assert_eq!(report.get("pe_trailing"), Some(&Value::Null)),
                "risk" if fixture.contains("negative") => {
                    assert_eq!(report.get("debt_to_equity"), Some(&Value::Null))
                }
                "fundamental" => {
                    assert_eq!(report["symbol"], "BBCA.JK");
                    assert_eq!(report["valuation"].get("pe_trailing"), Some(&Value::Null));
                }
                "compare" => {
                    assert_eq!(report.as_array().unwrap().len(), 2);
                    assert_eq!(report[0]["symbol"], "BBCA.JK");
                    assert_eq!(report[1]["symbol"], "BBRI.JK");
                    for row in report.as_array().unwrap() {
                        assert_eq!(row["valuation"].get("pe_trailing"), Some(&Value::Null));
                    }
                }
                "growth" => {
                    assert_eq!(report["revenue_growth_pct"].as_f64(), Some(8.1));
                    assert_eq!(report["earnings_growth_pct"].as_f64(), Some(12.1));
                }
                "risk" => assert_eq!(report["current_ratio"].as_f64(), Some(1.4)),
                _ => unreachable!(),
            }
        }
    }
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
fn cache_lifecycle_preserves_json_data_and_stderr_diagnostics() {
    for (command, provider) in [
        ("quote", "yahoo"),
        ("history", "yahoo"),
        ("technical", "yahoo"),
        ("profile", "msn"),
    ] {
        let root = test_env_dir(&format!("cache-lifecycle-{command}"));
        run_error_json(
            bin_with_root(&root)
                .env("IDX_PROVIDER", provider)
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args(["-o", "json", "--offline", "stocks", command, "BBCA"]),
            "CACHEMISS",
        );
        let warm = run_success_json(
            bin_with_root(&root)
                .env("IDX_PROVIDER", provider)
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .env("IDX_CACHE_QUOTE_TTL", "0")
                .env("IDX_CACHE_FUNDAMENTAL_TTL", "0")
                .args(["-o", "json", "stocks", command, "BBCA"]),
        );
        for offline in [true, false] {
            let mut cmd = bin_with_root(&root);
            cmd.env("IDX_PROVIDER", provider)
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .env("IDX_MOCK_ERROR", "1")
                .env("IDX_CACHE_QUOTE_TTL", "0")
                .env("IDX_CACHE_FUNDAMENTAL_TTL", "0")
                .args(["-o", "json"]);
            if offline {
                cmd.arg("--offline");
            }
            let output = cmd.args(["stocks", command, "BBCA"]).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(
                !output.stderr.is_empty(),
                "missing cache diagnostic for {command}"
            );
            let cached: Value = serde_json::from_slice(&output.stdout).expect("clean JSON stdout");
            assert_eq!(cached, warm, "{command}: offline={offline}");
        }
        run_error_json(
            bin_with_root(&root)
                .env("IDX_PROVIDER", provider)
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args(["-o", "json", "--offline", "stocks", command, "BBRI"]),
            "CACHEMISS",
        );
    }
}

#[test]
fn quote_and_compare_require_symbols() {
    test_bin("quote-no-symbols")
        .args(["stocks", "quote"])
        .assert()
        .failure();
    test_bin("compare-no-symbols")
        .args(["stocks", "compare"])
        .assert()
        .failure();
}

#[test]
fn version_and_cache_emit_json_in_json_mode() {
    let root = test_env_dir("json-meta");

    let version = run_success_stdout(bin_with_root(&root).args(["-o", "json", "version"]));
    let version: Value = serde_json::from_str(&version).expect("version json");
    assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));

    let empty = run_success_json(bin_with_root(&root).args(["-o", "json", "cache", "info"]));
    assert_eq!(empty["files"].as_u64(), Some(0));
    run_success_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "quote", "BBCA"]),
    );
    let populated = run_success_json(bin_with_root(&root).args(["-o", "json", "cache", "info"]));
    let files = populated["files"].as_u64().expect("cache file count");
    assert!(files > 0, "successful quote must populate the cache");
    let cleared = run_success_json(bin_with_root(&root).args(["-o", "json", "cache", "clear"]));
    assert_eq!(cleared["removed"].as_u64(), Some(files));
    let empty = run_success_json(bin_with_root(&root).args(["-o", "json", "cache", "info"]));
    assert_eq!(empty["files"].as_u64(), Some(0));
    run_error_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "--offline", "stocks", "quote", "BBCA"]),
        "CACHEMISS",
    );
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
fn config_set_and_get_ownership_snapshot_manifest_round_trip() {
    let root = test_env_dir("config-ownership-snapshot-manifest");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.snapshot_manifest",
            "/tmp/ownership-latest.json",
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["config", "get", "ownership.snapshot_manifest"])
        .assert()
        .success()
        .stdout(predicate::str::contains("/tmp/ownership-latest.json"));
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
    run_error_json(
        test_bin("ownership-fetch-bing-unsupported").args([
            "-o",
            "json",
            "ownership",
            "import",
            "--fetch-bing",
            "BBCA",
        ]),
        "UNSUPPORTED",
    );
}

#[test]
fn ownership_releases_uses_xdg_data_home_default_db_path() {
    let root = test_env_dir("ownership-xdg-data-home");
    let data_home = root.join("xdg-data");

    bin_with_root(&root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["ownership", "releases"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No ownership releases imported yet.",
        ));

    assert!(
        data_home.join("idx").join("ownership.db").exists(),
        "ownership db should be created under XDG_DATA_HOME when no custom path is configured"
    );
}

/// Keeps discovery tests off the live IDX data page (nothing listens on port 9).
const UNREACHABLE_DATA_PAGE_URL: &str = "http://127.0.0.1:9/data-kepemilikan-saham/";

#[test]
fn ownership_discover_finds_xlsx_on_share_ownership_page() {
    let page = fs::read_to_string("tests/fixtures/idx_share_ownership_page_excerpt.html")
        .expect("read share ownership page fixture");
    let page_url = spawn_single_response_server("text/html", page);

    let output = test_bin("ownership-discover-data-page")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env(
            "IDX_OWNERSHIP_ANNOUNCEMENT_API_URL",
            "http://127.0.0.1:9/announcements",
        )
        .env("IDX_OWNERSHIP_DATA_PAGE_URL", &page_url)
        .args(["-o", "json", "ownership", "discover", "--limit", "1"])
        .output()
        .expect("ownership discover json output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reports: Value = serde_json::from_slice(&output.stdout).expect("parse discover json");
    let latest = &reports[0];
    assert_eq!(latest["family"], "above_one_percent");
    assert_eq!(latest["status"], "supported");
    assert_eq!(latest["format"], "xlsx");
    assert_eq!(latest["as_of_date"], "2026-08-31");
    assert_eq!(
        latest["pdf_url"],
        "https://www.idx.co.id/Media/fahlw1o2/peng-2026-08-00017-satu-persen.xlsx"
    );
}

#[test]
fn ownership_discover_lists_fixture_candidates() {
    let body = fs::read_to_string("tests/fixtures/idx_announcement_kepemilikan.json")
        .expect("read ownership discovery fixture");
    let json_url = spawn_single_response_server("application/json", body);

    test_bin("ownership-discover")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env("IDX_OWNERSHIP_ANNOUNCEMENT_API_URL", &json_url)
        .env("IDX_OWNERSHIP_DATA_PAGE_URL", UNREACHABLE_DATA_PAGE_URL)
        .env(
            "IDX_OWNERSHIP_ANNOUNCEMENT_PAGE_URL",
            "http://127.0.0.1/pengumuman",
        )
        .args([
            "ownership",
            "discover",
            "--family",
            "above5",
            "--limit",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Above 5%"))
        .stdout(predicate::str::contains(
            "20260327_Semua Emiten Saham_Pengumuman Bursa_32055594.pdf",
        ))
        .stdout(predicate::str::contains(
            "20260327_Semua Emiten Saham_Pengumuman Bursa_32055594_lamp1.pdf",
        ));
}

#[test]
fn ownership_discover_supports_above1_family() {
    let body = r#"{
      "Items": [
        {
          "PublishDate": "2026-03-10T12:09:09",
          "Title": "Pemegang Saham di atas 1% (KSEI)",
          "AnnouncementType": "",
          "Code": "Semua Emiten Saham",
          "Attachments": [
            {
              "PDFFilename": "d67ebf37e6_10d4080288.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/d67ebf37e6_10d4080288.pdf",
              "IsAttachment": 0,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf"
            },
            {
              "PDFFilename": "b9b638e5a8_8928aca255.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf",
              "IsAttachment": 1,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf"
            }
          ],
          "PdfPath": ""
        }
      ],
      "ItemCount": 1,
      "PageSize": 10,
      "PageNumber": 1,
      "PageCount": 1
    }"#;
    let json_url = spawn_single_response_server("application/json", body.to_string());

    test_bin("ownership-discover-above1")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env("IDX_OWNERSHIP_ANNOUNCEMENT_API_URL", &json_url)
        .env("IDX_OWNERSHIP_DATA_PAGE_URL", UNREACHABLE_DATA_PAGE_URL)
        .env(
            "IDX_OWNERSHIP_ANNOUNCEMENT_PAGE_URL",
            "http://127.0.0.1/pengumuman",
        )
        .args([
            "ownership",
            "discover",
            "--family",
            "above1",
            "--limit",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Above 1%"))
        .stdout(predicate::str::contains(
            "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf",
        ))
        .stderr(predicate::str::contains("warning:"))
        .stderr(predicate::str::contains(
            "IDX share ownership data page discovery",
        ));
}

#[test]
fn ownership_discover_defaults_to_above1_and_prefers_supported_attachment() {
    let body = r#"{
      "Items": [
        {
          "PublishDate": "2026-03-10T12:09:09",
          "Title": "Pemegang Saham di atas 1% (KSEI)",
          "AnnouncementType": "",
          "Code": "Semua Emiten Saham",
          "Attachments": [
            {
              "PDFFilename": "d67ebf37e6_10d4080288.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/d67ebf37e6_10d4080288.pdf",
              "IsAttachment": 0,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf"
            },
            {
              "PDFFilename": "b9b638e5a8_8928aca255.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf",
              "IsAttachment": 1,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf"
            }
          ],
          "PdfPath": ""
        }
      ],
      "ItemCount": 1,
      "PageSize": 10,
      "PageNumber": 1,
      "PageCount": 1
    }"#;
    let json_url = spawn_single_response_server("application/json", body.to_string());

    let output = test_bin("ownership-discover-default")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env("IDX_OWNERSHIP_ANNOUNCEMENT_API_URL", &json_url)
        .env("IDX_OWNERSHIP_DATA_PAGE_URL", UNREACHABLE_DATA_PAGE_URL)
        .env(
            "IDX_OWNERSHIP_ANNOUNCEMENT_PAGE_URL",
            "http://127.0.0.1/pengumuman",
        )
        .args(["ownership", "discover", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8 stdout");
    assert!(stdout.contains("Above 1%"));
    assert!(stdout.contains("supported"));
    assert!(stdout.contains("20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf"));
    assert!(!stdout.contains("20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf\n"));
}

#[test]
fn ownership_discover_json_includes_status() {
    let body = r#"{
      "Items": [
        {
          "PublishDate": "2026-03-10T12:09:09",
          "Title": "Pemegang Saham di atas 1% (KSEI)",
          "AnnouncementType": "",
          "Code": "Semua Emiten Saham",
          "Attachments": [
            {
              "PDFFilename": "d67ebf37e6_10d4080288.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/d67ebf37e6_10d4080288.pdf",
              "IsAttachment": 0,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf"
            },
            {
              "PDFFilename": "b9b638e5a8_8928aca255.pdf",
              "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf",
              "IsAttachment": 1,
              "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf"
            }
          ],
          "PdfPath": ""
        }
      ],
      "ItemCount": 1,
      "PageSize": 10,
      "PageNumber": 1,
      "PageCount": 1
    }"#;
    let json_url = spawn_single_response_server("application/json", body.to_string());

    test_bin("ownership-discover-json-status")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env("IDX_OWNERSHIP_ANNOUNCEMENT_API_URL", &json_url)
        .env("IDX_OWNERSHIP_DATA_PAGE_URL", UNREACHABLE_DATA_PAGE_URL)
        .env(
            "IDX_OWNERSHIP_ANNOUNCEMENT_PAGE_URL",
            "http://127.0.0.1/pengumuman",
        )
        .args(["-o", "json", "ownership", "discover", "--limit", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"supported\""));
}

#[test]
fn ownership_import_url_rejects_html_response_before_pdf_parse() {
    let html_base = spawn_single_response_server(
        "text/html; charset=utf-8",
        "<!doctype html><html><body>blocked</body></html>",
    );
    let html_url = pdf_url(&html_base, "blocked");

    test_bin("ownership-import-url-html")
        .args(["ownership", "import", "--url", &html_url])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "returned HTML instead of a PDF/direct attachment",
        ));
}

#[test]
fn ownership_import_url_rejects_listing_page_inputs() {
    test_bin("ownership-import-url-listing-page")
        .args([
            "ownership",
            "import",
            "--url",
            "https://www.idx.co.id/id/berita/pengumuman/",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "ownership import --url accepts direct XLSX or PDF URLs only",
        ))
        .stderr(predicate::str::contains("ownership discover"));
}

#[test]
fn ownership_import_url_supported_pdf_succeeds_with_fake_mutool() {
    let root = test_env_dir("ownership-import-supported-remote");
    let db_path = root.join("ownership.db");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above1_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "supported");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args(["ownership", "import", "--url", &pdf_url])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 1 rows for 1 tickers"));

    bin_with_root(&root)
        .args(["ownership", "releases"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2026-02-27"))
        .stdout(predicate::str::contains(&pdf_url));
}

#[test]
fn ownership_import_url_caches_download_under_xdg_cache_home() {
    let root = test_env_dir("ownership-import-xdg-cache");
    let db_path = root.join("ownership.db");
    let cache_home = root.join("xdg-cache");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above1_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "cached-raw");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .env("XDG_CACHE_HOME", &cache_home)
        .args(["ownership", "import", "--url", &pdf_url])
        .assert()
        .success();

    assert!(
        cache_home
            .join("idx")
            .join("ownership")
            .join("raw")
            .join("cached-raw.pdf")
            .exists(),
        "downloaded remote PDF should be cached under XDG_CACHE_HOME"
    );
}

#[test]
fn ownership_import_url_duplicate_release_is_skipped() {
    let root = test_env_dir("ownership-import-duplicate-release");
    let db_path = root.join("ownership.db");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above1_stext_excerpt.xml"),
    );
    let first_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let second_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let first_url = pdf_url(&first_base, "supported-first");
    let second_url = pdf_url(&second_base, "supported-second");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args(["ownership", "import", "--url", &first_url])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args(["ownership", "import", "--url", &second_url])
        .assert()
        .success()
        .stdout(predicate::str::contains("Release already imported"));
}

#[test]
fn ownership_import_file_xlsx_supports_ticker_and_releases() {
    let root = test_env_dir("ownership-import-xlsx");
    let db_path = root.join("ownership.db");
    let xlsx_path = root.join("peng-2026-08-00017-satu-persen.xlsx");
    fs::write(
        &xlsx_path,
        include_bytes!("fixtures/ksei_above1_20260831_excerpt.xlsx"),
    )
    .expect("write xlsx fixture");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let imported = run_success_json(bin_with_root(&root).args([
        "-o",
        "json",
        "ownership",
        "import",
        "--file",
        xlsx_path.to_str().unwrap(),
    ]));
    assert_eq!(imported["as_of_date"], "2026-08-31");
    assert_eq!(imported["inserted_rows"].as_u64(), Some(17));
    let ticker =
        run_success_json(bin_with_root(&root).args(["-o", "json", "ownership", "ticker", "BBCA"]));
    assert_eq!(ticker["ksei_as_of"], "2026-08-31");
    assert!(
        ticker["holders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "DWIMURIA INVESTAMA ANDALAN"
                && row["percentage_bps"].as_i64() == Some(5494))
    );
    let releases =
        run_success_json(bin_with_root(&root).args(["-o", "json", "ownership", "releases"]));
    assert_eq!(releases.as_array().unwrap().len(), 1);
    assert_eq!(releases[0]["as_of_date"], "2026-08-31");
    assert_eq!(releases[0]["row_count"].as_u64(), Some(17));
}

#[test]
fn ownership_import_file_rejects_above5_xlsx() {
    let root = test_env_dir("ownership-import-xlsx-above5");
    let db_path = root.join("ownership.db");
    let xlsx_path = root.join("peng-2026-09-23-00080-lima-persen.xlsx");
    fs::write(
        &xlsx_path,
        include_bytes!("fixtures/ksei_above5_20260923_excerpt.xlsx"),
    )
    .expect("write xlsx fixture");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["ownership", "import", "--file", xlsx_path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("above-1% holder register layout"));
}

#[test]
fn ownership_import_force_reimports_existing_release() {
    let root = test_env_dir("ownership-import-force");
    let db_path = root.join("ownership.db");
    let pdf_path = root.join("force.pdf");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above1_stext_excerpt.xml"),
    );
    fs::write(&pdf_path, fake_pdf_bytes()).expect("write local pdf fixture");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args(["ownership", "import", "--file", pdf_path.to_str().unwrap()])
        .assert()
        .success();

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args([
            "ownership",
            "import",
            "--force",
            "--file",
            pdf_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 1 rows for 1 tickers"));

    let output = bin_with_root(&root)
        .args(["-o", "json", "ownership", "releases"])
        .output()
        .expect("ownership releases json output");
    assert!(output.status.success());
    let releases: Value = serde_json::from_slice(&output.stdout).expect("parse releases json");
    assert_eq!(releases.as_array().unwrap().len(), 1);
}

#[test]
fn ownership_import_url_rejects_legacy_above5_pdf_schema() {
    let root = test_env_dir("ownership-import-above5-unsupported");
    let data_home = root.join("data");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above5_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "legacy-above5");

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .env("XDG_DATA_HOME", &data_home)
        .args(["ownership", "import", "--url", &pdf_url])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy IDX `above5` ownership PDFs are not supported for import",
        ));
}

#[test]
fn ownership_import_url_rejects_legacy_investor_type_pdf_schema() {
    let root = test_env_dir("ownership-import-investor-type-unsupported");
    let data_home = root.join("data");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_investor_type_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "legacy-investor-type");

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .env("XDG_DATA_HOME", &data_home)
        .args(["ownership", "import", "--url", &pdf_url])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy IDX `investor-type` ownership PDFs are not supported for import",
        ));
}

#[test]
fn ownership_import_file_txt_archive_succeeds() {
    let root = test_env_dir("ownership-import-txt-archive");
    let db_path = root.join("ownership.db");
    let txt_path = root.join("Balancepos20260227.txt");
    fs::write(
        &txt_path,
        include_str!("fixtures/ksei_balancepos_20260227_excerpt.txt"),
    )
    .expect("write txt archive fixture");

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&root)
        .args(["ownership", "import", "--file", txt_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 18 rows for 1 tickers"));

    bin_with_root(&root)
        .args(["ownership", "ticker", "AADI", "--source", "ksei"])
        .assert()
        .success()
        .stdout(predicate::str::contains("KSEI AGGREGATE LOCAL CP"))
        .stdout(predicate::str::contains("64.67%"));
}

#[test]
fn ownership_import_file_zip_archive_supports_releases_ticker_and_changes() {
    let root = test_env_dir("ownership-import-zip-archive");
    let db_path = root.join("ownership.db");
    let jan_zip = root.join("BalanceposEfek20260130.zip");
    let feb_zip = root.join("BalanceposEfek20260227.zip");

    write_zip_with_text(
        &jan_zip,
        "Balancepos20260130.txt",
        include_str!("fixtures/ksei_balancepos_20260130_excerpt.txt"),
    );
    write_zip_with_text(
        &feb_zip,
        "Balancepos20260227.txt",
        include_str!("fixtures/ksei_balancepos_20260227_excerpt.txt"),
    );

    bin_with_root(&root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    for (file, date) in [(&jan_zip, "2026-01-30"), (&feb_zip, "2026-02-27")] {
        let imported = run_success_json(bin_with_root(&root).args([
            "-o",
            "json",
            "ownership",
            "import",
            "--file",
            file.to_str().unwrap(),
        ]));
        assert_eq!(imported["inserted_rows"].as_u64(), Some(18));
        assert_eq!(imported["ticker_count"].as_u64(), Some(1));
        assert_eq!(imported["as_of_date"], date);
    }
    let releases =
        run_success_json(bin_with_root(&root).args(["-o", "json", "ownership", "releases"]));
    assert_eq!(releases.as_array().unwrap().len(), 2);
    assert_eq!(releases[0]["as_of_date"], "2026-02-27");
    assert_eq!(releases[1]["as_of_date"], "2026-01-30");
    let ticker = run_success_json(bin_with_root(&root).args([
        "-o",
        "json",
        "ownership",
        "ticker",
        "AADI",
        "--source",
        "ksei",
    ]));
    let holders = ticker["holders"].as_array().expect("holders array");
    assert_eq!(ticker["ksei_as_of"].as_str(), Some("2026-02-27"));
    assert_eq!(holders.len(), 18);
    assert_eq!(ticker["concentration"]["top1_bps"].as_i64(), Some(6467));
    assert!(holders.iter().any(|row| {
        row["name"].as_str() == Some("KSEI AGGREGATE LOCAL CP")
            && row["percentage_bps"].as_i64() == Some(6467)
    }));
    assert!(!holders.iter().any(|row| {
        row["name"].as_str() == Some("KSEI AGGREGATE LOCAL CP")
            && row["percentage_bps"].as_i64() == Some(6481)
    }));

    let changes = run_success_json(bin_with_root(&root).args([
        "-o",
        "json",
        "ownership",
        "changes",
        "--from",
        "2026-01-30",
        "--to",
        "2026-02-27",
    ]));
    let local_cp = changes
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["ticker_code"] == "AADI" && row["entity_name"] == "KSEI AGGREGATE LOCAL CP")
        .unwrap();
    assert_eq!(local_cp["change_type"], "decreased");
    assert_eq!(local_cp["old_bps"].as_i64(), Some(6481));
    assert_eq!(local_cp["new_bps"].as_i64(), Some(6467));
    assert_eq!(local_cp["delta_bps"].as_i64(), Some(-14));
}

#[test]
fn ownership_sync_downloads_snapshot_larger_than_ten_mib_over_http() {
    // ureq's default 10 MiB body limit broke `ownership sync` for every
    // client when a 15.3 MB snapshot (5 months of history) was published.
    let publisher_root = test_env_dir("ownership-sync-large-publisher");
    let (source_db, manifest_path) = prepare_snapshot_fixture(&publisher_root);
    {
        let conn = Connection::open(&source_db).expect("open snapshot db");
        conn.execute_batch("CREATE TABLE test_padding (data BLOB)")
            .expect("create padding table");
        conn.execute(
            "INSERT INTO test_padding (data) VALUES (zeroblob(?1))",
            [11 * 1024 * 1024],
        )
        .expect("insert padding");
    }
    let snapshot_bytes = fs::read(&source_db).expect("read padded snapshot");
    assert!(snapshot_bytes.len() > 10 * 1024 * 1024);

    write_snapshot_manifest(&manifest_path, &source_db, None);
    let snapshot_url = spawn_single_response_server("application/octet-stream", snapshot_bytes);
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    manifest["snapshot"]["download_url"] = Value::String(format!("{snapshot_url}/snapshot.sqlite"));
    fs::write(&manifest_path, manifest.to_string()).expect("write http manifest");

    let sync_root = test_env_dir("ownership-sync-large-consumer");
    let target_db = sync_root.join("ownership.db");
    bin_with_root(&sync_root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            target_db.to_str().unwrap(),
        ])
        .assert()
        .success();

    bin_with_root(&sync_root)
        .args([
            "ownership",
            "sync",
            "--manifest",
            manifest_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed ownership snapshot"));
}

#[test]
fn ownership_sync_rejects_body_larger_than_manifest_size() {
    let publisher_root = test_env_dir("ownership-sync-oversized-body");
    let (source_db, manifest_path) = prepare_snapshot_fixture(&publisher_root);
    write_snapshot_manifest(&manifest_path, &source_db, None);
    let mut body = fs::read(&source_db).expect("read snapshot");
    body.extend_from_slice(&[0u8; 4096]);
    let snapshot_url = spawn_single_response_server("application/octet-stream", body);
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    manifest["snapshot"]["download_url"] = Value::String(format!("{snapshot_url}/snapshot.sqlite"));
    fs::write(&manifest_path, manifest.to_string()).expect("write http manifest");

    let sync_root = test_env_dir("ownership-sync-oversized-body-consumer");
    bin_with_root(&sync_root)
        .args([
            "config",
            "set",
            "ownership.db_path",
            sync_root.join("ownership.db").to_str().unwrap(),
        ])
        .assert()
        .success();
    bin_with_root(&sync_root)
        .args([
            "ownership",
            "sync",
            "--manifest",
            manifest_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "larger than the manifest's size_bytes",
        ));
}

#[test]
fn ownership_sync_installs_snapshot_and_preserves_query_behavior() {
    let publisher_root = test_env_dir("ownership-sync-publisher");
    let (source_db, manifest_path) = prepare_snapshot_fixture(&publisher_root);
    let sync_root = test_env_dir("ownership-sync-consumer");
    let target_db = sync_root.join("ownership.db");
    run_success_stdout(bin_with_root(&sync_root).args([
        "config",
        "set",
        "ownership.db_path",
        target_db.to_str().unwrap(),
    ]));
    run_success_stdout(bin_with_root(&sync_root).args([
        "config",
        "set",
        "ownership.snapshot_manifest",
        manifest_path.to_str().unwrap(),
    ]));
    let queries = [
        vec!["-o", "json", "ownership", "releases"],
        vec![
            "-o",
            "json",
            "ownership",
            "ticker",
            "AADI",
            "--source",
            "ksei",
        ],
        vec![
            "-o",
            "json",
            "ownership",
            "changes",
            "--from",
            "2026-01-31",
            "--to",
            "2026-02-27",
        ],
    ];
    let expected: Vec<Value> = queries
        .iter()
        .map(|args| run_success_json(bin_with_root(&publisher_root).args(args)))
        .collect();
    assert_eq!(expected[0].as_array().unwrap().len(), 2);
    assert_eq!(expected[0][0]["as_of_date"], "2026-02-27");
    assert_eq!(expected[1]["ksei_as_of"], "2026-02-27");
    assert!(
        expected[1]["holders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "ADARO STRATEGIC INVESTMENTS"
                && row["percentage_bps"].as_i64() == Some(4110))
    );
    assert!(
        expected[2]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["ticker_code"] == "AADI"
                && row["change_type"] == "increased"
                && row["delta_bps"].as_i64() == Some(128))
    );
    for (force, action) in [
        (false, "installed"),
        (false, "no_change"),
        (true, "refreshed"),
    ] {
        let mut cmd = bin_with_root(&sync_root);
        cmd.args(["-o", "json", "ownership", "sync"]);
        if force {
            cmd.arg("--force");
        }
        let result = run_success_json(&mut cmd);
        assert_eq!(result["action"], action);
        assert_eq!(result["snapshot_version"], "2026-02-27");
        assert_eq!(result["release_count"].as_u64(), Some(2));
        for (args, expected) in queries.iter().zip(&expected) {
            assert_eq!(
                run_success_json(bin_with_root(&sync_root).args(args)),
                *expected
            );
        }
    }

    let bytes = fs::read(&target_db).unwrap();
    // Both failures happen after a usable local snapshot has been installed.
    write_snapshot_manifest(
        &manifest_path,
        &source_db,
        Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
    );
    run_error_json(
        bin_with_root(&sync_root).args(["-o", "json", "ownership", "sync", "--force"]),
        "PARSEERROR",
    );
    assert_eq!(fs::read(&target_db).unwrap(), bytes);
    for (args, expected) in queries.iter().zip(&expected) {
        assert_eq!(
            run_success_json(bin_with_root(&sync_root).args(args)),
            *expected
        );
    }

    // A matching checksum must not make an invalid downloaded database installable.
    let invalid = b"not a SQLite database";
    let invalid_url = spawn_single_response_server("application/octet-stream", invalid.to_vec());
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["snapshot"]["download_url"] = invalid_url.into();
    manifest["snapshot"]["sqlite_sha256"] = sha256_hex(invalid).into();
    manifest["snapshot"]["size_bytes"] = invalid.len().into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    run_error_json(
        bin_with_root(&sync_root).args(["-o", "json", "ownership", "sync", "--force"]),
        "DATABASEERROR",
    );
    assert_eq!(fs::read(&target_db).unwrap(), bytes);
    for (args, expected) in queries.iter().zip(&expected) {
        assert_eq!(
            run_success_json(bin_with_root(&sync_root).args(args)),
            *expected
        );
    }
}

#[test]
fn quiet_suppresses_non_essential_history_messages() {
    let root = test_env_dir("quiet-history-messages");
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
        .args(["--quiet", "stocks", "technical", "BBCA"])
        .assert()
        .success()
        .stderr(predicate::str::contains("info:").not())
        .stderr(predicate::str::contains("warning:").not());
}

#[test]
fn cache_namespace_isolated_by_provider() {
    let root = test_env_dir("provider-cache");
    let yahoo = run_success_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "yahoo")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "quote", "BBCA"]),
    );
    run_error_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "--offline", "stocks", "quote", "BBCA"]),
        "CACHEMISS",
    );
    run_error_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .env("IDX_MOCK_ERROR", "1")
            .args(["-o", "json", "stocks", "quote", "BBCA"]),
        "PROVIDERUNAVAILABLE",
    );
    let msn = run_success_json(
        bin_with_root(&root)
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "quote", "BBCA"]),
    );
    assert_ne!(
        yahoo, msn,
        "provider-specific percentage values must remain distinct"
    );
    for (provider, expected) in [("yahoo", yahoo), ("msn", msn)] {
        assert_eq!(
            run_success_json(
                bin_with_root(&root)
                    .env("IDX_PROVIDER", provider)
                    .env("IDX_USE_MOCK_PROVIDER", "1")
                    .env("IDX_MOCK_ERROR", "1")
                    .args(["-o", "json", "--offline", "stocks", "quote", "BBCA"])
            ),
            expected
        );
    }
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

#[test]
fn invalid_provider_env_honors_json_output() {
    run_error_json(
        test_bin("invalid-provider-json")
            .env("IDX_PROVIDER", "bogus")
            .args(["-o", "json", "version"]),
        "CONFIGERROR",
    );
}

#[test]
fn invalid_quote_ttl_env_returns_non_zero() {
    test_bin("invalid-quote-ttl")
        .env("IDX_CACHE_QUOTE_TTL", "bogus")
        .args(["version"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid IDX_CACHE_QUOTE_TTL value",
        ));
}

#[test]
fn invalid_fundamental_ttl_env_honors_json_output() {
    run_error_json(
        test_bin("invalid-fundamental-ttl-json")
            .env("IDX_CACHE_FUNDAMENTAL_TTL", "bogus")
            .args(["-o", "json", "version"]),
        "CONFIGERROR",
    );
}

#[test]
fn offline_and_no_cache_flags_are_rejected() {
    run_error_json(
        test_bin("offline-no-cache").args([
            "-o",
            "json",
            "--offline",
            "--no-cache",
            "stocks",
            "quote",
            "BBCA",
        ]),
        "INVALIDINPUT",
    );
}

#[test]
fn msn_screen_rejects_invalid_filter() {
    test_bin("msn-screen-invalid-filter")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "screen", "--filter", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid screener filter"));
}

#[test]
fn msn_screen_rejects_invalid_region_in_json_mode() {
    run_error_json(
        test_bin("msn-screen-invalid-region-json")
            .env("IDX_PROVIDER", "msn")
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .args(["-o", "json", "stocks", "screen", "--region", "eu"]),
        "INVALIDINPUT",
    );
}

#[test]
fn verbose_history_surfaces_yahoo_dropped_row_diagnostics() {
    let history_fixture = fixture_path("chart_bbca_with_gap.json");

    test_bin("verbose-history-diagnostics")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_MOCK_YAHOO_HISTORY_FIXTURE", &history_fixture)
        .args(["-v", "stocks", "history", "BBCA", "--period", "3mo"])
        .assert()
        .success()
        .stderr(predicate::str::contains("dropped 1 OHLC row(s)"));
}

#[test]
fn yahoo_provider_rejects_msn_only_stock_commands() {
    let cases = [
        (
            "yahoo-profile-unsupported",
            vec!["stocks", "profile", "BBCA"],
        ),
        (
            "yahoo-financials-unsupported",
            vec!["stocks", "financials", "BBCA"],
        ),
        (
            "yahoo-earnings-unsupported",
            vec!["stocks", "earnings", "BBCA"],
        ),
        (
            "yahoo-sentiment-unsupported",
            vec!["stocks", "sentiment", "BBCA"],
        ),
        (
            "yahoo-insights-unsupported",
            vec!["stocks", "insights", "BBCA"],
        ),
        ("yahoo-news-unsupported", vec!["stocks", "news", "BBCA"]),
        ("yahoo-screen-unsupported", vec!["stocks", "screen"]),
    ];

    for (name, args) in cases {
        run_error_json(
            test_bin(name)
                .env("IDX_PROVIDER", "yahoo")
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .args(["-o", "json"])
                .args(args),
            "UNSUPPORTED",
        );
    }
}

#[test]
fn industry_only_msn_mock_fundamentals_are_rejected_for_analysis_commands() {
    let fixture = fixture_path("msn_keyratios_industry_only.json");
    let fixture_str = fixture
        .to_str()
        .expect("fixture path should be valid unicode")
        .to_string();
    let cases = [
        ("growth-industry-only", vec!["stocks", "growth", "BBCA"]),
        (
            "valuation-industry-only",
            vec!["stocks", "valuation", "BBCA"],
        ),
        ("risk-industry-only", vec!["stocks", "risk", "BBCA"]),
        (
            "fundamental-industry-only",
            vec!["stocks", "fundamental", "BBCA"],
        ),
        (
            "compare-industry-only",
            vec!["stocks", "compare", "BBCA,BBRI"],
        ),
    ];

    for (name, args) in cases {
        run_error_json(
            test_bin(name)
                .env("IDX_PROVIDER", "msn")
                .env("IDX_USE_MOCK_PROVIDER", "1")
                .env("IDX_MOCK_MSN_KEYRATIOS_FIXTURE", &fixture_str)
                .args(["-o", "json"])
                .args(args),
            "PARSEERROR",
        );
    }
}

fn write_large_screener_fixture(root: &Path, count: Option<usize>) -> PathBuf {
    let quotes: Vec<_> = (0..503)
        .map(|i| {
            serde_json::json!({
                "symbol": format!("S{i:04}"),
                "price": 1000,
                "pricePreviousClose": 900,
                "priceChangePercent": ((i + 2) % 503) as i64 - 251,
                "accumulatedVolume": 1000 + (i + 1) % 503,
                "marketCap": 1000000 - (i + 1) % 503,
                "timeLastTraded": "2026-01-02T09:00:00Z"
            })
        })
        .collect();
    let mut raw = serde_json::json!({ "quote": quotes });
    if let Some(count) = count {
        raw["count"] = serde_json::json!(count);
    }
    let path = root.join("screener.json");
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    path
}

fn screener_fixture_command(root: &Path, fixture: &Path, filter: &str, limit: usize) -> Command {
    let mut cmd = bin_with_root(root);
    cmd.env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_MOCK_MSN_SCREENER_FIXTURE", fixture)
        .args([
            "-o", "json", "stocks", "screen", "--filter", filter, "--limit",
        ])
        .arg(limit.to_string());
    cmd
}

fn screener_symbols(value: &serde_json::Value) -> Vec<&str> {
    value
        .as_array()
        .expect("screen array")
        .iter()
        .map(|quote| quote["symbol"].as_str().expect("normalized symbol"))
        .collect()
}

#[test]
fn msn_screen_ranks_complete_candidates_and_caches_before_limit() {
    let cases: [(&str, Vec<usize>); 4] = [
        (
            "top-performers",
            (0..=500).rev().chain([502, 501]).collect(),
        ),
        (
            "worst-performers",
            [501, 502].into_iter().chain(0..=500).collect(),
        ),
        ("high-volume", (0..=501).rev().chain([502]).collect()),
        ("large-cap", [502].into_iter().chain(0..=501).collect()),
    ];
    for (filter, indices) in cases {
        let root = test_env_dir(&format!("screen-complete-{filter}"));
        let fixture = write_large_screener_fixture(&root, Some(503));
        let expected: Vec<_> = indices.iter().map(|i| format!("S{i:04}.JK")).collect();
        // A tiny first request must cache all candidates, including beyond row 500.
        let small = run_success_json(&mut screener_fixture_command(&root, &fixture, filter, 3));
        assert_eq!(screener_symbols(&small), expected[..3], "{filter}");
        assert_eq!(small[0]["price"], 1000);
        assert_eq!(small[0]["change"], 100);
        fs::remove_file(&fixture).unwrap();
        let mut full_cmd = screener_fixture_command(&root, &fixture, filter, 600);
        full_cmd.env("IDX_MOCK_ERROR", "1").arg("--offline");
        let full = run_success_json(&mut full_cmd);
        assert_eq!(screener_symbols(&full), expected, "{filter}");
        let rows = full.as_array().unwrap();
        for limit in [1, 3, 17, 500, 503, 600] {
            let mut cmd = screener_fixture_command(&root, &fixture, filter, limit);
            cmd.env("IDX_MOCK_ERROR", "1").arg("--offline");
            let offline = run_success_json(&mut cmd);
            assert_eq!(
                offline.as_array().unwrap(),
                &rows[..limit.min(503)],
                "{filter}: {limit}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn msn_screen_rejects_invalid_totals_without_populating_cache() {
    for (name, count) in [("missing", None), ("incomplete", Some(504))] {
        let root = test_env_dir(&format!("screen-total-{name}"));
        let fixture = write_large_screener_fixture(&root, count);
        run_error_json(
            &mut screener_fixture_command(&root, &fixture, "top-performers", 3),
            "PARSEERROR",
        );
        let mut offline = screener_fixture_command(&root, &fixture, "top-performers", 3);
        run_error_json(offline.arg("--offline"), "CACHEMISS");
        write_large_screener_fixture(&root, Some(503));
        let recovered = run_success_json(&mut screener_fixture_command(
            &root,
            &fixture,
            "top-performers",
            600,
        ));
        let expected: Vec<_> = (0..=500)
            .rev()
            .chain([502, 501])
            .map(|i| format!("S{i:04}.JK"))
            .collect();
        assert_eq!(screener_symbols(&recovered), expected);
        fs::remove_dir_all(root).unwrap();
    }
}
