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

pub mod auth;
pub mod exodus;
pub mod loopback;
mod parse;

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use parse::{parse_symbol_page, status_error, stockbit_code};

use super::types::{Bar, Fundamentals, Interval, Intraday, OrderBook, Period, Quote};
use super::{
    FundamentalsProvider, HistoryProvider, IntradayProvider, OrderBookProvider, QuoteProvider,
};
use crate::error::IdxError;
use auth::SessionStore;
use exodus::Exodus;

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

/// Quotes come from the user's session when one is stored (live, from
/// `exodus`), else from the public page. The order book and history need the
/// session; intraday always reads the public page.
pub struct StockbitProvider {
    agent: ureq::Agent,
    /// `None` when the config directory cannot be resolved.
    exodus: Option<Exodus>,
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
        Self {
            agent,
            exodus: SessionStore::open().ok().map(Exodus::new),
        }
    }

    /// The session API when the user has a session.
    fn session(&self) -> Option<&Exodus> {
        self.exodus.as_ref().filter(|e| e.store().has_session())
    }

    fn require_session(&self) -> Result<&Exodus, IdxError> {
        self.session()
            .ok_or_else(|| IdxError::AuthError(auth::NO_SESSION.to_string()))
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
pub(crate) fn pace() {
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
        match self.session() {
            Some(exodus) => exodus.quote(symbol),
            None => self.snapshot(symbol).map(|day| day.quote),
        }
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

impl OrderBookProvider for StockbitProvider {
    fn orderbook(&self, symbol: &str) -> Result<OrderBook, IdxError> {
        self.require_session()?.orderbook(symbol)
    }
}

impl HistoryProvider for StockbitProvider {
    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        self.require_session()?.daily(symbol, period, interval)
    }
}

/// Whether the user has a Stockbit session stored or seeded.
pub fn has_session() -> bool {
    SessionStore::open().is_ok_and(|store| store.has_session())
}

pub(crate) fn fundamentals_unsupported(symbol: &str) -> IdxError {
    IdxError::Unsupported(format!(
        "{symbol}: Stockbit has no fundamentals; command requires --provider msn or yahoo"
    ))
}

/// Stores a pasted session (stockbit.com's `credentialStorage` cookie or a
/// refresh token) and checks it with one request, which refreshes it when the
/// access token is missing or stale. A session Stockbit refuses is removed.
pub fn login(pasted: &str) -> Result<auth::SessionStatus, IdxError> {
    let session = auth::session_from_paste(pasted).ok_or_else(|| {
        IdxError::InvalidInput(
            "not a Stockbit session: paste the value of stockbit.com's `credentialStorage` \
             cookie, or a refresh token"
                .to_string(),
        )
    })?;
    let store = SessionStore::open()?;
    // A refused paste must not lose a session that already worked.
    let previous = std::fs::read(store.path()).ok();
    store.save(&session)?;
    #[cfg(feature = "testing")]
    let check = !crate::api::mock::enabled();
    #[cfg(not(feature = "testing"))]
    let check = true;
    if check && let Err(err) = Exodus::new(store.clone()).verify() {
        restore(&store, previous);
        return Err(err);
    }
    Ok(store.status(chrono::Utc::now().timestamp()))
}

fn restore(store: &SessionStore, previous: Option<Vec<u8>>) {
    match previous {
        Some(bytes) => {
            let _ = std::fs::write(store.path(), bytes);
        }
        None => {
            let _ = store.clear();
        }
    }
}

/// The stored session's state, without a request and without any token.
pub fn session_status() -> Result<auth::SessionStatus, IdxError> {
    Ok(SessionStore::open()?.status(chrono::Utc::now().timestamp()))
}

/// Deletes the stored session: whether there was one, and the state after
/// (still logged in when `IDX_STOCKBIT_*` seeds one).
pub fn logout() -> Result<(bool, auth::SessionStatus), IdxError> {
    let store = SessionStore::open()?;
    let removed = store.clear()?;
    Ok((removed, store.status(chrono::Utc::now().timestamp())))
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
