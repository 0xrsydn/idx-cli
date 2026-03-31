use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;

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

fn prepend_path(dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(current) if !current.is_empty() => format!("{}:{current}", dir.display()),
        _ => dir.display().to_string(),
    }
}

fn pdf_url(base: &str, name: &str) -> String {
    format!("{base}/{name}.pdf")
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
        .stdout(predicate::str::contains("PT Bank Central Asia Tbk"))
        .stdout(predicate::str::contains("Financials"))
        .stdout(predicate::str::contains("https://www.bca.co.id/"));
}

#[test]
fn msn_profile_with_mock_fixture_json_prefers_company_and_localized_fields() {
    test_bin("msn-profile-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "profile", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"long_name\": \"PT Bank Central Asia Tbk\"",
        ))
        .stdout(predicate::str::contains(
            "\"industry\": \"Banking Services\"",
        ))
        .stdout(predicate::str::contains("\"country\": \"Indonesia\""))
        .stdout(predicate::str::contains("Indonesia-based commercial bank"));
}

#[test]
fn msn_financials_with_mock_fixture_table_contains_sections() {
    test_bin("msn-financials-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "financials", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Income Statement (2025-12-31)"))
        .stdout(predicate::str::contains("Net Income"))
        .stdout(predicate::str::contains("Operating Cash Flow"))
        .stdout(predicate::str::contains("Cash Flow"));
}

#[test]
fn msn_earnings_with_mock_fixture_table_is_sectioned_and_formatted() {
    test_bin("msn-earnings-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "earnings", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Earnings History"))
        .stdout(predicate::str::contains("Earnings Forecast"))
        .stdout(predicate::str::contains("FY2025"))
        .stdout(predicate::str::contains("Q1 2026"))
        .stdout(predicate::str::contains("110,000,000,000"))
        .stdout(predicate::str::contains("2026-03-15"));
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
fn msn_sentiment_with_mock_fixture_json_contains_symbol_and_counts() {
    test_bin("msn-sentiment-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "sentiment", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\": \"BBCA.JK\""))
        .stdout(predicate::str::contains("\"time_range\": \"1D\""))
        .stdout(predicate::str::contains("\"bullish\": 10"))
        .stdout(predicate::str::contains("\"neutral\": 3"));
}

#[test]
fn msn_insights_with_mock_fixture_table_contains_highlights_and_risks() {
    test_bin("msn-insights-table")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "insights", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Mixed analyst signals"))
        .stdout(predicate::str::contains(
            "Last updated: 2026-03-26T04:14:57.9197955Z",
        ))
        .stdout(predicate::str::contains("Highlights:"))
        .stdout(predicate::str::contains(
            "Analyst price target: Analysts forecast more than 20% upside",
        ))
        .stdout(predicate::str::contains("Risks:"))
        .stdout(predicate::str::contains(
            "Quarterly Revenue YoY Growth: Revenue grew worse than peers",
        ));
}

#[test]
fn msn_insights_with_mock_fixture_json_contains_summary_and_last_updated() {
    test_bin("msn-insights-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "insights", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"summary\": \"Mixed analyst signals",
        ))
        .stdout(predicate::str::contains(
            "\"last_updated\": \"2026-03-26T04:14:57.9197955Z\"",
        ))
        .stdout(predicate::str::contains("Revenue grew worse than peers"));
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
fn msn_news_with_mock_fixture_json_contains_provider_and_timestamp() {
    test_bin("msn-news-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "news", "BBCA", "--limit", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"news-1\""))
        .stdout(predicate::str::contains(
            "\"title\": \"BCA reports steady growth\"",
        ))
        .stdout(predicate::str::contains("\"provider\": \"Contoso News\""))
        .stdout(predicate::str::contains(
            "\"published_at\": \"2026-03-20T10:00:00Z\"",
        ));
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
fn msn_screen_with_mock_fixture_json_contains_normalized_symbols_and_ranges() {
    test_bin("msn-screen-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args([
            "-o",
            "json",
            "stocks",
            "screen",
            "--filter",
            "top-performers",
            "--limit",
            "10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\": \"BBCA.JK\""))
        .stdout(predicate::str::contains("\"symbol\": \"BBRI.JK\""))
        .stdout(predicate::str::contains("\"change\": 117"))
        .stdout(predicate::str::contains("\"range_signal\": \"upper\""));
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
fn ownership_discover_lists_fixture_candidates() {
    let body = fs::read_to_string("tests/fixtures/idx_announcement_kepemilikan.json")
        .expect("read ownership discovery fixture");
    let json_url = spawn_single_response_server("application/json", body);

    test_bin("ownership-discover")
        .env("IDX_CURL_IMPERSONATE_BIN", "curl")
        .env("IDX_OWNERSHIP_ANNOUNCEMENT_API_URL", &json_url)
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
            "ownership import --url accepts direct PDF URLs only",
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
fn ownership_import_url_rejects_legacy_above5_pdf_schema() {
    let root = test_env_dir("ownership-import-above5-unsupported");
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_above5_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "legacy-above5");

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
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
    let fake_mutool_dir = install_fake_mutool(
        &root,
        include_str!("fixtures/ksei_investor_type_stext_excerpt.xml"),
    );
    let pdf_base = spawn_single_response_server("application/pdf", fake_pdf_bytes());
    let pdf_url = pdf_url(&pdf_base, "legacy-investor-type");

    bin_with_root(&root)
        .env("PATH", prepend_path(&fake_mutool_dir))
        .args(["ownership", "import", "--url", &pdf_url])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy IDX `investor-type` ownership PDFs are not supported for import",
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

#[test]
fn invalid_provider_env_honors_json_output() {
    test_bin("invalid-provider-json")
        .env("IDX_PROVIDER", "bogus")
        .args(["-o", "json", "version"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"error\": true"))
        .stderr(predicate::str::contains("invalid provider"));
}

#[test]
fn offline_and_no_cache_flags_are_rejected() {
    test_bin("offline-no-cache")
        .args(["--offline", "--no-cache", "stocks", "quote", "BBCA"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot combine --offline with --no-cache",
        ));
}

#[test]
fn msn_profile_supports_offline_cache_reads() {
    let root = test_env_dir("msn-profile-offline");
    let cache_home = root.join("cache");

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["stocks", "profile", "BBCA"])
        .assert()
        .success();

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["--offline", "stocks", "profile", "BBCA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PT Bank Central Asia Tbk"));
}

#[test]
fn msn_profile_serves_stale_cache_on_provider_failure_with_warning() {
    let root = test_env_dir("msn-profile-stale");
    let cache_home = root.join("cache");

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_FUNDAMENTAL_TTL", "0")
        .args(["stocks", "profile", "BBCA"])
        .assert()
        .success();

    bin_with_root(&root)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .env("IDX_CACHE_FUNDAMENTAL_TTL", "0")
        .env("IDX_MOCK_ERROR", "1")
        .args(["stocks", "profile", "BBCA"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: network failed"))
        .stdout(predicate::str::contains("PT Bank Central Asia Tbk"));
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
    test_bin("msn-screen-invalid-region-json")
        .env("IDX_PROVIDER", "msn")
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .args(["-o", "json", "stocks", "screen", "--region", "eu"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"error\": true"))
        .stderr(predicate::str::contains("invalid screener region"));
}
