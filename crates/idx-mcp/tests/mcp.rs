//! Drives the idx-mcp binary over stdio JSON-RPC with the mock provider and a
//! fixture-built ownership database.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use idx_core::config::CoreConfig;
use idx_core::investor::{ImportSource, Investor};
use serde_json::{Value, json};

const TOOLS: [&str; 11] = [
    "analyze_stock",
    "compare_stocks",
    "get_company",
    "get_investor_holdings",
    "get_ownership_changes",
    "get_price_history",
    "get_quotes",
    "get_shareholders",
    "ownership_status",
    "screen_stocks",
    "search_investors",
];

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    _home: tempdir::Dir,
}

impl Client {
    fn start(args: &[&str], seed: bool) -> Self {
        let home = tempdir::Dir::new();
        if seed {
            seed_ownership_db(&home.path().join("data/idx/ownership.db"));
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_idx-mcp"))
            .args(args)
            // The mock provider reads `tests/fixtures/...` relative to here.
            .current_dir(core_dir())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("IDX_USE_MOCK_PROVIDER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn idx-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            _home: home,
        };
        let init = client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "idx-mcp");
        client.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        client
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").expect("write request");
        self.stdin.flush().expect("flush request");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read response");
            assert!(read > 0, "idx-mcp closed stdout");
            let message: Value = serde_json::from_str(&line).expect("JSON-RPC line");
            if message["id"] == id {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }

    fn tool_names(&mut self) -> Vec<String> {
        let result = self.request("tools/list", json!({}));
        let mut names: Vec<String> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        names.sort();
        names
    }

    /// Calls a tool, returning `(is_error, structured_content)`.
    fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let structured = result["structuredContent"].clone();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), structured);
        (result["isError"] == true, structured)
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let (is_error, body) = self.call(name, arguments);
        assert!(!is_error, "{name} failed: {body}");
        body
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The above-1% XLSX (2026-08-31) and two KSEI archive months (2026-01-30,
/// 2026-02-27), as in the CLI golden tests.
fn seed_ownership_db(db: &Path) {
    let fixtures = core_dir().join("tests/fixtures");
    let staging = db.parent().unwrap().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    // The importer derives the release from the IDX file name.
    let xlsx = staging.join("peng-2026-08-00017-satu-persen.xlsx");
    std::fs::copy(fixtures.join("ksei_above1_20260831_excerpt.xlsx"), &xlsx).unwrap();

    let investor = Investor::new(&CoreConfig {
        ownership_db: Some(db.to_path_buf()),
        ..CoreConfig::default()
    });
    for file in [
        xlsx,
        fixtures.join("ksei_balancepos_20260130_excerpt.txt"),
        fixtures.join("ksei_balancepos_20260227_excerpt.txt"),
    ] {
        investor
            .import(&ImportSource::File(file), false)
            .expect("seed import");
    }
}

fn core_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../idx-core")
        .canonicalize()
        .expect("idx-core crate dir")
}

fn assert_fetched(value: &Value) {
    assert!(value.get("data").is_some(), "{value}");
    assert_eq!(value["stale"], false, "{value}");
    assert!(value["source"].is_string(), "{value}");
    assert!(value["fetched_at"].is_string(), "{value}");
}

#[test]
fn lists_grouped_tools_and_hides_sync_unless_allowed() {
    let mut client = Client::start(&[], false);
    assert_eq!(client.tool_names(), TOOLS);

    let mut client = Client::start(&["--allow-sync"], false);
    let mut expected: Vec<&str> = TOOLS.to_vec();
    expected.push("ownership_sync");
    expected.sort();
    assert_eq!(client.tool_names(), expected);
}

#[test]
fn live_tools_wrap_data_with_freshness() {
    let mut client = Client::start(&[], false);

    let quotes = client.ok("get_quotes", json!({"symbols": ["BBCA"]}));
    assert_fetched(&quotes["quotes"][0]);
    assert_eq!(quotes["quotes"][0]["data"]["symbol"], "BBCA.JK");

    let history = client.ok("get_price_history", json!({"symbol": "BBCA"}));
    assert_fetched(&history);
    assert!(!history["data"].as_array().unwrap().is_empty());

    let screen = client.ok("screen_stocks", json!({"limit": 1}));
    assert_fetched(&screen);
    assert_eq!(screen["data"].as_array().unwrap().len(), 1);

    let company = client.ok("get_company", json!({"symbol": "BBCA"}));
    assert_eq!(company["symbol"], "BBCA");
    for section in [
        "profile",
        "financials",
        "earnings",
        "sentiment",
        "insights",
        "news",
    ] {
        assert_fetched(&company[section]);
    }
    assert!(company.get("errors").is_none(), "{company}");

    let only_profile = client.ok(
        "get_company",
        json!({"symbol": "BBCA", "sections": ["profile"]}),
    );
    assert!(only_profile.get("financials").is_none());

    let analysis = client.ok("analyze_stock", json!({"symbol": "BBCA"}));
    for kind in ["technical", "growth", "valuation", "risk"] {
        assert_fetched(&analysis[kind]);
    }

    let compared = client.ok("compare_stocks", json!({"symbols": ["BBCA", "BBRI"]}));
    assert_eq!(compared["reports"].as_array().unwrap().len(), 2);
    assert_eq!(compared["failures"], json!([]));
}

#[test]
fn errors_carry_cli_codes() {
    let mut client = Client::start(&[], false);

    let (is_error, body) = client.call(
        "get_price_history",
        json!({"symbol": "BBCA", "period": "7y"}),
    );
    assert!(is_error);
    assert_eq!(body["code"], "INVALIDINPUT");
    assert_eq!(body["error"], true);

    let (is_error, body) = client.call("screen_stocks", json!({"filter": "cheap"}));
    assert!(is_error);
    assert_eq!(body["code"], "INVALIDINPUT");

    // Empty ownership database: investor tools point at the sync path.
    let status = client.ok("ownership_status", json!({}));
    assert_eq!(status["populated"], false);
    let (is_error, body) = client.call("get_shareholders", json!({"ticker": "BBCA"}));
    assert!(is_error);
    assert_eq!(body["code"], "NOMARKETDATA");
    assert!(body["message"].as_str().unwrap().contains("--allow-sync"));
}

#[test]
fn ownership_tools_read_the_local_database() {
    let mut client = Client::start(&[], true);

    let status = client.ok("ownership_status", json!({}));
    assert_eq!(status["populated"], true);
    assert_eq!(status["latest_as_of_date"], "2026-08-31");
    assert_eq!(status["releases"].as_array().unwrap().len(), 3);

    let holders = client.ok("get_shareholders", json!({"ticker": "bbca"}));
    assert_eq!(holders["ticker"]["code"], "BBCA");
    assert!(!holders["holders"].as_array().unwrap().is_empty());

    let (is_error, body) = client.call("get_shareholders", json!({"ticker": "ZZZZ"}));
    assert!(is_error, "{body}");

    let holdings = client.ok("get_investor_holdings", json!({"entity": "DWIMURIA"}));
    assert!(
        !holdings["holdings"].as_array().unwrap().is_empty(),
        "{holdings}"
    );

    let found = client.ok("search_investors", json!({"query": "DWIMURIA"}));
    assert!(!found["entities"].as_array().unwrap().is_empty(), "{found}");

    let all = client.ok(
        "get_ownership_changes",
        json!({"from": "2026-01-30", "to": "2026-02-27"}),
    );
    let total = all["total"].as_u64().unwrap();
    assert!(total > 1, "{all}");
    let ticker = all["changes"][0]["ticker_code"]
        .as_str()
        .unwrap()
        .to_string();

    let limited = client.ok(
        "get_ownership_changes",
        json!({"from": "2026-01-30", "to": "2026-02-27", "limit": 1}),
    );
    assert_eq!(limited["total"], total);
    assert_eq!(limited["changes"].as_array().unwrap().len(), 1);

    let one = client.ok(
        "get_ownership_changes",
        json!({"from": "2026-01-30", "to": "2026-02-27", "ticker": ticker.to_lowercase()}),
    );
    let rows = one["changes"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row["ticker_code"] == ticker.as_str()));
}

/// A temporary directory removed on drop (no extra dependency).
mod tempdir {
    use super::*;

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "idx-mcp-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
