//! Stockbit's public symbol page: a quote, the best bid/offer and today's
//! price per minute for one IDX symbol, without a login or key.
//!
//! `GET https://stockbit.com/symbol/{CODE}` server-renders everything into
//! the page's `__NEXT_DATA__` JSON (see `parse`). Stockbit's own JSON API
//! (`exodus.stockbit.com`) needs a logged-in bearer token and is not used.
//!
//! Access rules this module keeps:
//! - An honest user agent. CloudFront in front of the page answers a static
//!   HTTP 403 to `curl/`, `axios/`, `Go-http-client/` and empty agents; it
//!   does not fingerprint TLS, so no browser impersonation is involved.
//! - One request per symbol, at least `MIN_INTERVAL` apart across every idx
//!   process of the user (see `pace`), no retries, and a 403 surfaces as
//!   `UPSTREAMBLOCKED` at once.
//! - Stockbit's terms describe its market data as delayed at least 10
//!   minutes and for personal use; frontends say so.

mod parse;

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use parse::{parse_symbol_page, status_error, stockbit_code};

use super::types::{Fundamentals, Intraday, Quote};
use super::{FundamentalsProvider, IntradayProvider, QuoteProvider};
use crate::error::IdxError;

pub const SOURCE: &str = "stockbit";
/// Identifies idx-cli and where to reach its maintainers.
pub const USER_AGENT: &str = concat!(
    "idx-cli/",
    env!("CARGO_PKG_VERSION"),
    " (+",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);
/// Longest a snapshot is served from cache: enough to coalesce a burst (an
/// agent asking for a quote and the intraday series), short enough that a
/// session view stays current.
pub(crate) const CACHE_TTL_SECS: u64 = 5;
/// Least time between two requests to stockbit.com from one process.
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const BASE_URL: &str = "https://stockbit.com/symbol/";

pub struct StockbitProvider {
    agent: ureq::Agent,
}

impl StockbitProvider {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            // A server that stalls before sending headers must not hold a
            // call (and an MCP call slot) forever.
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        Self { agent }
    }

    /// The quote and today's series for a resolved symbol (`BBCA.JK`).
    pub fn snapshot(&self, resolved: &str) -> Result<Intraday, IdxError> {
        let code = stockbit_code(resolved)?;
        let html = self.page(&code, resolved)?;
        parse_symbol_page(resolved, &html)
    }

    fn page(&self, code: &str, resolved: &str) -> Result<String, IdxError> {
        pace();
        let response = self
            .agent
            .get(format!("{BASE_URL}{code}"))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "text/html")
            .call();
        match response {
            Ok(ok) => ok
                .into_body()
                .read_to_string()
                .map_err(|e| IdxError::Http(format!("stockbit: {e}"))),
            Err(ureq::Error::StatusCode(status)) => Err(status_error(status, resolved)),
            Err(err) => Err(IdxError::Http(format!("stockbit: {err}"))),
        }
    }
}

impl Default for StockbitProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Waits until `MIN_INTERVAL` has passed since the previous request. Threads
/// of one process queue on a mutex; separate processes (a shell loop of
/// `idx` calls, a CLI beside idx-mcp) share the time of the last request in
/// `<cache>/stockbit.pace` under an exclusive file lock. When that file is
/// unusable, pacing is per process.
fn pace() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(prev) = *last {
        let wait = MIN_INTERVAL.saturating_sub(prev.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    // Best effort: the in-process pacing above already holds.
    let _ = pace_across_processes();
    *last = Some(Instant::now());
}

fn pace_across_processes() -> std::io::Result<()> {
    let dir = crate::cache::cache_dir().map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("stockbit.pace"))?;
    // Released when `file` is dropped, after the new time is written.
    file.lock()?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    if let Ok(prev_ms) = raw.trim().parse::<u128>() {
        // A clock that went backwards waits at most one interval.
        let elapsed = now_ms().saturating_sub(prev_ms);
        let wait = MIN_INTERVAL.as_millis().saturating_sub(elapsed);
        if wait > 0 {
            std::thread::sleep(Duration::from_millis(wait as u64));
        }
    }
    stamp(&mut file)
}

fn stamp(file: &mut File) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    write!(file, "{}", now_ms())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl QuoteProvider for StockbitProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        self.snapshot(symbol).map(|day| day.quote)
    }
}

impl FundamentalsProvider for StockbitProvider {
    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        Err(fundamentals_unsupported(symbol))
    }
}

impl IntradayProvider for StockbitProvider {
    fn intraday(&self, symbol: &str) -> Result<Intraday, IdxError> {
        self.snapshot(symbol)
    }
}

pub(crate) fn fundamentals_unsupported(symbol: &str) -> IdxError {
    IdxError::Unsupported(format!(
        "{symbol}: Stockbit has no fundamentals; command requires --provider msn or yahoo"
    ))
}

#[cfg(test)]
mod tests {
    use super::USER_AGENT;

    /// CloudFront answers HTTP 403 to agents containing these (probed
    /// 2026-10-22); the honest agent must never match one.
    #[test]
    fn user_agent_is_honest_and_not_on_the_cdn_blocklist() {
        assert!(USER_AGENT.starts_with("idx-cli/"));
        assert!(USER_AGENT.contains("github.com/0xrsydn/idx-cli"));
        for blocked in ["curl/", "axios/", "Go-http-client/"] {
            assert!(
                !USER_AGENT.contains(blocked),
                "{USER_AGENT} contains {blocked}"
            );
        }
    }
}
