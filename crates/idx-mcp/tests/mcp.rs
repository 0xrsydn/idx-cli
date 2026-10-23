//! Drives the idx-mcp binary over stdio JSON-RPC with the mock provider and a
//! fixture-built ownership database.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use idx_core::config::CoreConfig;
use idx_core::investor::{ImportSource, Investor};
use serde_json::{Value, json};

const TOOLS: [&str; 17] = [
    "analyze_bandar",
    "analyze_stock",
    "compare_stocks",
    "get_broker_summary",
    "get_company",
    "get_foreign_flow",
    "get_intraday",
    "get_investor_holdings",
    "get_orderbook",
    "get_ownership_changes",
    "get_price_history",
    "get_quotes",
    "get_shareholders",
    "ownership_status",
    "screen_stocks",
    "search_investors",
    "stockbit_status",
];

/// An unsigned test token expiring in 2100, as a seeded Stockbit session.
const TEST_SESSION: (&str, &str) = (
    "IDX_STOCKBIT_REFRESH_TOKEN",
    "eyJhbGciOiJub25lIn0.eyJleHAiOiA0MTAyNDQ0ODAwLCAibm90ZSI6ICJ0ZXN0IHRva2VuLCBzaWduZWQgYnkgbm9ib2R5In0.c2ln",
);

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    _home: tempdir::Dir,
}

impl Client {
    fn start(args: &[&str], seed: bool) -> Self {
        Self::start_with_env(args, seed, &[])
    }

    fn start_with_env(args: &[&str], seed: bool, env: &[(&str, &str)]) -> Self {
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
            .envs(env.iter().copied())
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

/// What agents see (names, descriptions, input schemas) is part of the
/// contract: changes must show up as a reviewed diff. Regenerate with
/// `UPDATE_GOLDEN=1`.
#[test]
fn tool_definitions_match_golden() {
    let mut client = Client::start(&["--allow-sync"], false);
    let mut tools = client.request("tools/list", json!({}))["tools"]
        .as_array()
        .unwrap()
        .clone();
    tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let rendered = serde_json::to_string_pretty(&tools).unwrap() + "\n";

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tools.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &rendered).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).expect("tests/golden/tools.json");
    assert!(
        rendered == expected,
        "tool definitions changed; review and rerun with UPDATE_GOLDEN=1:\n{rendered}"
    );
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

    let brokers = client.ok(
        "get_broker_summary",
        json!({"symbol": "BBCA", "date": "2026-10-16"}),
    );
    assert_fetched(&brokers);
    assert_eq!(brokers["source"], "ipot");
    assert_eq!(brokers["data"]["buyers"][0]["code"], "MG");
    assert_eq!(brokers["data"]["sellers"][0]["name"], "MANDIRI SEKURITAS");
    assert_eq!(brokers["data"]["foreign_net_value"], -2_300_000_000_000_i64);

    let bandar = client.ok(
        "analyze_bandar",
        json!({"symbols": ["BBCA", "BBRI", "ZZZZ"], "date": "2026-10-16", "top": 3}),
    );
    let reports = bandar["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 2);
    assert_fetched(&reports[0]);
    assert_eq!(reports[0]["data"]["top"], 3);
    assert_eq!(reports[0]["data"]["method"], "top_concentration_v1");
    assert!(
        reports[0]["data"]["imbalance_bps"].as_i64()
            >= reports[1]["data"]["imbalance_bps"].as_i64()
    );
    assert_eq!(bandar["failures"][0]["symbol"], "ZZZZ.JK");
    assert_eq!(bandar["failures"][0]["code"], "SYMBOLNOTFOUND");

    let flow = client.ok(
        "get_foreign_flow",
        json!({"symbol": "BBCA", "from": "2026-10-12", "to": "2026-10-16"}),
    );
    assert_fetched(&flow);
    assert_eq!(flow["source"], "ipot");
    assert_eq!(flow["data"]["days"].as_array().unwrap().len(), 5);
    assert_eq!(
        flow["data"]["days"][4]["foreign_sell_value"],
        4_000_000_000_000_i64
    );
    let idx_flow = client.ok(
        "get_foreign_flow",
        json!({"symbol": "BBCA", "from": "2026-10-12", "to": "2026-10-16", "source": "idx"}),
    );
    assert_eq!(idx_flow["source"], "idx");
    assert_eq!(idx_flow["data"]["days"][4]["close"], 5175);

    let compared = client.ok("compare_stocks", json!({"symbols": ["BBCA", "BBRI"]}));
    assert_eq!(compared["reports"].as_array().unwrap().len(), 2);
    assert_eq!(compared["failures"], json!([]));
}

#[test]
fn intraday_is_stockbit_whichever_provider_and_empty_sessions_are_data() {
    let mut client = Client::start(&[], false);

    let day = client.ok("get_intraday", json!({"symbol": "BBCA"}));
    assert_fetched(&day);
    assert_eq!(day["source"], "stockbit");
    let quote = &day["data"]["quote"];
    assert_eq!(quote["symbol"], "BBCA.JK");
    assert_eq!(quote["market_status"], "close");
    // The closing book is not reported as a live one.
    assert!(
        quote.get("bid").is_none() && quote.get("offer").is_none(),
        "{quote}"
    );
    assert_eq!(day["data"]["points"].as_array().unwrap().len(), 335);

    let tail = client.ok("get_intraday", json!({"symbol": "BBCA", "tail": 2}));
    let points = tail["data"]["points"].as_array().unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[1]["time"], "2026-10-22T16:14:00+07:00");

    let suspended = client.ok("get_intraday", json!({"symbol": "MABA"}));
    assert_eq!(suspended["data"]["points"], json!([]));
    assert_eq!(
        suspended["data"]["quote"]["session_note"],
        "Stockbit lists the stock as suspended"
    );

    for (arguments, code) in [
        (json!({"symbol": "ZZZZ"}), "SYMBOLNOTFOUND"),
        (json!({"symbol": "BBCA", "tail": 0}), "INVALIDINPUT"),
        (json!({"symbol": "BBCA", "tail": 401}), "INVALIDINPUT"),
    ] {
        let (is_error, body) = client.call("get_intraday", arguments.clone());
        assert!(is_error, "{arguments}: {body}");
        assert_eq!(body["code"], code, "{arguments}: {body}");
    }
}

#[test]
fn stockbit_provider_quotes_carry_market_status_and_blocks_surface_once() {
    let mut client = Client::start_with_env(&[], false, &[("IDX_PROVIDER", "stockbit")]);
    let quotes = client.ok("get_quotes", json!({"symbols": ["BBCA"]}));
    assert_eq!(quotes["quotes"][0]["source"], "stockbit");
    assert_eq!(quotes["quotes"][0]["data"]["market_status"], "close");

    let mut blocked = Client::start_with_env(&[], false, &[("IDX_MOCK_STOCKBIT_STATUS", "403")]);
    let (is_error, body) = blocked.call("get_intraday", json!({"symbol": "BBCA"}));
    assert!(is_error, "{body}");
    assert_eq!(body["code"], "UPSTREAMBLOCKED");
    assert!(
        body["message"].as_str().unwrap().contains("does not retry"),
        "{body}"
    );
}

#[test]
fn the_order_book_needs_a_session_and_status_never_shows_tokens() {
    let mut client = Client::start(&[], false);
    let status = client.ok("stockbit_status", json!({}));
    assert_eq!(status["logged_in"], false);
    let (is_error, body) = client.call("get_orderbook", json!({"symbol": "BBCA"}));
    assert!(is_error, "{body}");
    assert_eq!(body["code"], "AUTHERROR");

    let mut client = Client::start_with_env(&[], false, &[TEST_SESSION]);
    let status = client.ok("stockbit_status", json!({}));
    assert_eq!(status["logged_in"], true);
    assert!(!status.to_string().contains(TEST_SESSION.1));

    let book = client.ok("get_orderbook", json!({"symbol": "BBCA", "depth": 2}));
    assert_fetched(&book);
    assert_eq!(book["source"], "stockbit");
    let offers = book["data"]["offers"].as_array().unwrap();
    assert_eq!(offers.len(), 2);
    assert_eq!(
        offers[0],
        json!({"price": 5175, "shares": 264_462_500, "orders": 3284})
    );
    assert_eq!(book["data"]["foreign_net"], -2_329_493_087_500_i64);
    assert_eq!(book["data"]["market_status"], "STATE_NAME_MARKET_CLOSED");
    for depth in [0, 51] {
        let (is_error, body) =
            client.call("get_orderbook", json!({"symbol": "BBCA", "depth": depth}));
        assert!(is_error, "{body}");
        assert_eq!(body["code"], "INVALIDINPUT");
    }
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

    for (tool, arguments) in [
        ("get_quotes", json!({"symbols": []})),
        ("compare_stocks", json!({"symbols": []})),
        ("get_company", json!({"symbol": "BBCA", "sections": []})),
        ("analyze_stock", json!({"symbol": "BBCA", "kinds": []})),
        (
            "get_broker_summary",
            json!({"symbol": "BBCA", "date": "2026-10-16", "from": "2026-10-12"}),
        ),
        (
            "get_broker_summary",
            json!({"symbol": "BBCA", "date": "2026-10-16", "board": "dark"}),
        ),
        ("analyze_bandar", json!({"symbols": ["BBCA"], "top": 4})),
        ("analyze_bandar", json!({"symbols": []})),
        ("get_foreign_flow", json!({"symbol": "BBCA", "days": 61})),
        (
            "get_foreign_flow",
            json!({"symbol": "BBCA", "source": "rti"}),
        ),
        (
            "get_foreign_flow",
            json!({"symbol": "BBCA", "days": 3, "from": "2026-10-12"}),
        ),
    ] {
        let (is_error, body) = client.call(tool, arguments);
        assert!(is_error, "{tool}: {body}");
        assert_eq!(body["code"], "INVALIDINPUT", "{tool}");
    }

    let many: Vec<String> = (0..21).map(|i| format!("S{i}")).collect();
    for (tool, arguments) in [
        ("get_quotes", json!({"symbols": many})),
        ("screen_stocks", json!({"limit": 201})),
        ("screen_stocks", json!({"limit": 0})),
        ("get_quotes", json!({"symbols": ["../../REPORT"]})),
    ] {
        let (is_error, body) = client.call(tool, arguments.clone());
        assert!(is_error, "{tool} {arguments}: {body}");
        assert_eq!(body["code"], "INVALIDINPUT", "{tool} {arguments}");
    }
    // Duplicates collapse instead of counting against the cap.
    let repeated = vec!["BBCA"; 50];
    let quotes = client.ok("get_quotes", json!({"symbols": repeated}));
    assert_eq!(quotes["quotes"].as_array().unwrap().len(), 1);

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
    assert_eq!(limited["next_offset"], 1);

    // Pages cover every row exactly once, largest change first.
    let mut paged = Vec::new();
    let mut offset = Some(0);
    while let Some(next) = offset {
        let page = client.ok(
            "get_ownership_changes",
            json!({"from": "2026-01-30", "to": "2026-02-27", "limit": 2, "offset": next}),
        );
        paged.extend(page["changes"].as_array().unwrap().iter().cloned());
        // The summary covers all rows, so every page carries the same one.
        assert_eq!(page["summary"], all["summary"], "offset {next}");
        offset = page["next_offset"].as_u64();
    }
    assert_eq!(paged.len() as u64, total);
    assert_eq!(&paged, all["changes"].as_array().unwrap());
    let deltas: Vec<_> = paged
        .iter()
        .map(|row| row["delta_bps"].as_i64().unwrap().abs())
        .collect();
    assert!(
        deltas.windows(2).all(|pair| pair[0] >= pair[1]),
        "{deltas:?}"
    );

    let by_type = all["summary"]["by_type"].as_object().unwrap();
    assert_eq!(
        by_type.values().map(|n| n.as_u64().unwrap()).sum::<u64>(),
        total
    );
    assert!(all["summary"]["tickers"].as_u64().unwrap() >= 1, "{all}");

    // Past the end: an empty last page, not an error.
    for past in [total, total + 5] {
        let page = client.ok(
            "get_ownership_changes",
            json!({"from": "2026-01-30", "to": "2026-02-27", "offset": past}),
        );
        assert_eq!(page["total"], total, "offset {past}");
        assert_eq!(
            page["changes"].as_array().unwrap().len(),
            0,
            "offset {past}"
        );
        assert!(page["next_offset"].is_null(), "offset {past}: {page}");
    }

    // Bad, unknown, or reversed dates are errors, not "everyone is new".
    for (from, to) in [
        ("", "2026-02-27"),
        ("1999-01-01", "2026-02-27"),
        ("2026-02-27", "2026-01-30"),
        ("2026-01-30", "2026-01-30"),
    ] {
        let (is_error, body) =
            client.call("get_ownership_changes", json!({"from": from, "to": to}));
        assert!(is_error, "{from}..{to}: {body}");
        assert_eq!(body["code"], "INVALIDINPUT", "{from}..{to}: {body}");
    }
    let (is_error, body) = client.call("get_investor_holdings", json!({"entity": "  "}));
    assert!(is_error, "{body}");
    assert_eq!(body["code"], "INVALIDINPUT", "{body}");

    let (is_error, body) = client.call(
        "get_ownership_changes",
        json!({"from": "2026-01-30", "to": "2026-02-27", "limit": 301}),
    );
    assert!(is_error, "{body}");
    assert_eq!(body["code"], "INVALIDINPUT");
    assert!(body.to_string().contains("offset"), "{body}");

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
