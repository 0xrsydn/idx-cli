//! Stockbit's own market-data API, `https://exodus.stockbit.com`, with the
//! user's session (see `auth`). Opt-in: used only when a session is stored.
//!
//! - `GET /company-price-feed/v2/orderbook/companies/{CODE}`: last price,
//!   OHLC, volume (shares), value, frequency, foreign buy/sell/net (IDR, used
//!   only when `has_foreign_bs` is true) and the depth ladder
//!   `bid[]`/`offer[]` of `{price, que_num, volume}` (shares).
//! - `GET /company-price-feed/market-time/session`: the market's state,
//!   `data.detail.regular.state_name`: `STATE_NAME_SESSION_2` while trading,
//!   `STATE_NAME_BREAK`, `STATE_NAME_MARKET_CLOSED` (recorded). The `fca`
//!   (full call auction) board beside it keeps its own state and is not read.
//! - `GET /charts/{CODE}/daily?timeframe=1m|3m|1y&chart_type=PRICE_CHART_TYPE_CANDLE`:
//!   daily candles, `formatted_date` `YYYY-MM-DD`, string OHLC, `value` the
//!   close and `volume` in lots.
//! - `POST /login/refresh` with the refresh token as bearer rotates the pair.
//!
//! A 401 (or a JSON 403) spends one refresh and retries once. Requests share
//! the public page's pacing. Endpoints and units were mapped by fzn0x/idx
//! (MIT); `tests/fixtures/stockbit_exodus_*` are its recordings.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{Months, NaiveDate};
use serde_json::Value;

use super::auth::SessionStore;
use super::parse::number;
use super::{USER_AGENT, pace, stockbit_code};
use super::{corpaction, movement};
use crate::api::types::{BookLevel, Interval, Ohlc, OrderBook, Period, Quote};
use crate::corporate::types::{ActionKind, CorporateAction};
use crate::error::IdxError;
use crate::movements::types::{MovementPage, MovementQuery};

const BASE_URL: &str = "https://exodus.stockbit.com";
const REFRESH_PATH: &str = "/login/refresh";
/// The prefix of the states in which the regular market trades:
/// `STATE_NAME_SESSION_2` was recorded on 2026-11-03 from 13:30; session 1 is
/// assumed to follow the same naming. Every other state (`STATE_NAME_BREAK`,
/// `STATE_NAME_MARKET_CLOSED`, auctions) hides bid/offer in quotes, as on the
/// public page.
const TRADING_PREFIX: &str = "STATE_NAME_SESSION_";

/// Whether Stockbit's market state is a trading session.
pub fn is_trading(state: &str) -> bool {
    state
        .strip_prefix(TRADING_PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
/// The market's state changes a few times a day; one read serves this long.
const MARKET_STATE_TTL: Duration = Duration::from_secs(30);

pub(crate) struct Exodus {
    agent: ureq::Agent,
    store: SessionStore,
}

struct Reply {
    status: u16,
    body: String,
}

impl Reply {
    /// The server refused the access token: 401, or a 403 with a JSON body
    /// (a CDN block is an HTML page).
    fn rejected(&self) -> bool {
        self.status == 401
            || (self.status == 403 && serde_json::from_str::<Value>(&self.body).is_ok())
    }
}

impl Exodus {
    pub(crate) fn new(store: SessionStore) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent, store }
    }

    pub(crate) fn store(&self) -> &SessionStore {
        &self.store
    }

    /// One authorised request (the market's state), refreshing the session
    /// when needed: proves Stockbit accepts it.
    pub(crate) fn verify(&self) -> Result<(), IdxError> {
        self.get("/company-price-feed/market-time/session", "market session")
            .map(|_| ())
    }

    pub(crate) fn orderbook(&self, resolved: &str) -> Result<OrderBook, IdxError> {
        let code = stockbit_code(resolved)?;
        let body = self.get(
            &format!("/company-price-feed/v2/orderbook/companies/{code}"),
            resolved,
        )?;
        parse_orderbook(resolved, &body, self.market_status())
    }

    pub(crate) fn quote(&self, resolved: &str) -> Result<Quote, IdxError> {
        self.orderbook(resolved).map(|book| quote_from_book(&book))
    }

    /// Daily candles covering `period`, oldest first.
    pub(crate) fn daily(
        &self,
        resolved: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        let window =
            history_window(period, interval).ok_or_else(|| unsupported_window(period, interval))?;
        let code = stockbit_code(resolved)?;
        let body = self.get(
            &format!("/charts/{code}/daily?timeframe={window}&chart_type=PRICE_CHART_TYPE_CANDLE"),
            resolved,
        )?;
        daily_for_period(&body, period)
    }

    /// Corporate actions of `kind`: one stock's (`resolved`) or the market's.
    pub(crate) fn corporate_actions(
        &self,
        kind: ActionKind,
        resolved: Option<&str>,
    ) -> Result<Vec<CorporateAction>, IdxError> {
        let path = format!("/corpaction/{}", corpaction::path_type(kind));
        let (path, subject) = match resolved {
            Some(resolved) => (
                format!("{path}?symbol={}", stockbit_code(resolved)?),
                resolved.to_string(),
            ),
            None => (path, "corporate actions".to_string()),
        };
        let body = self.get(&path, &subject)?;
        corpaction::parse_corporate_actions(kind, &body)
    }

    /// Page `page` of shareholder movements: one stock's (`query.symbol`,
    /// resolved) or the market's.
    pub(crate) fn shareholder_movements(
        &self,
        query: &MovementQuery,
        page: u32,
    ) -> Result<MovementPage, IdxError> {
        let code = query.symbol.as_deref().map(stockbit_code).transpose()?;
        let subject = query
            .symbol
            .clone()
            .unwrap_or_else(|| "shareholder movements".to_string());
        let body = self.get(
            &movement::request_path(query, code.as_deref(), page),
            &subject,
        )?;
        movement::parse_movements(&body)
    }

    /// The market's state, read at most every `MARKET_STATE_TTL` per process.
    /// Unknown on any failure, which hides bid/offer.
    fn market_status(&self) -> Option<String> {
        static CACHE: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
        let mut cached = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, state)) = cached.as_ref()
            && at.elapsed() < MARKET_STATE_TTL
        {
            return state.clone();
        }
        let state = self
            .get("/company-price-feed/market-time/session", "market session")
            .ok()
            .and_then(|body| parse_market_status(&body));
        *cached = Some((Instant::now(), state.clone()));
        state
    }

    fn get(&self, path: &str, subject: &str) -> Result<String, IdxError> {
        let refresher = |refresh: &str| self.refresh(refresh);
        let token = self.store.access(&refresher, unix_now())?;
        let mut reply = self.send(path, &token)?;
        if reply.rejected() {
            let token = self.store.after_rejection(&token, &refresher)?;
            reply = self.send(path, &token)?;
            if reply.rejected() {
                return Err(IdxError::AuthError(
                    "Stockbit rejected the session; run `idx stockbit login` again".to_string(),
                ));
            }
        }
        classify(reply, subject)
    }

    fn send(&self, path: &str, token: &str) -> Result<Reply, IdxError> {
        pace();
        let response = self
            .agent
            .get(format!("{BASE_URL}{path}"))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .header("Origin", "https://stockbit.com")
            .header("Referer", "https://stockbit.com/")
            .header("Authorization", format!("Bearer {token}"))
            .call()
            .map_err(|e| IdxError::Http(format!("stockbit api: {e}")))?;
        read_reply(response)
    }

    /// `POST /login/refresh`; the response body carries the new pair.
    fn refresh(&self, refresh: &str) -> Result<String, IdxError> {
        pace();
        let response = self
            .agent
            .post(format!("{BASE_URL}{REFRESH_PATH}"))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", "https://stockbit.com")
            .header("Referer", "https://stockbit.com/")
            .header("Authorization", format!("Bearer {refresh}"))
            .send("{}")
            .map_err(|e| IdxError::Http(format!("stockbit refresh: {e}")))?;
        let reply = read_reply(response)?;
        match reply.status {
            200..=299 => Ok(reply.body),
            401 | 403 => Err(IdxError::AuthError(
                "Stockbit rejected the refresh token (expired or revoked); run `idx stockbit \
                 login` again"
                    .to_string(),
            )),
            status => Err(IdxError::Http(format!("stockbit refresh: HTTP {status}"))),
        }
    }
}

fn read_reply(response: ureq::http::Response<ureq::Body>) -> Result<Reply, IdxError> {
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| IdxError::Http(format!("stockbit api: {e}")))?;
    Ok(Reply { status, body })
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A non-rejected reply as data or the error its status means.
fn classify(reply: Reply, subject: &str) -> Result<String, IdxError> {
    match reply.status {
        200..=299 => Ok(reply.body),
        404 if error_type(&reply.body).as_deref() == Some("NOT_FOUND") => {
            Err(IdxError::SymbolNotFound(subject.to_string()))
        }
        429 => Err(IdxError::RateLimited),
        403 => Err(blocked()),
        status => Err(IdxError::Http(format!(
            "stockbit api: HTTP {status}{}",
            message(&reply.body)
                .map(|m| format!(": {m}"))
                .unwrap_or_default()
        ))),
    }
}

fn error_type(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("error_type")?
        .as_str()
        .map(str::to_string)
}

fn message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

fn data(body: &str, what: &str) -> Result<Value, IdxError> {
    let mut value: Value = serde_json::from_str(body)
        .map_err(|e| IdxError::ParseError(format!("stockbit {what}: {e}")))?;
    match value.get_mut("data") {
        Some(data) => Ok(data.take()),
        None => Err(IdxError::ParseError(format!("stockbit {what}: no data"))),
    }
}

fn whole(value: Option<&Value>) -> Option<i64> {
    value.and_then(number).map(|n| n.round() as i64)
}

fn count(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(number)
        .filter(|n| *n >= 0.0)
        .map(|n| n.round() as u64)
}

/// The order book and session figures; `market_status` comes from the
/// market-time endpoint.
pub fn parse_orderbook(
    resolved: &str,
    body: &str,
    market_status: Option<String>,
) -> Result<OrderBook, IdxError> {
    let d = data(body, "order book")?;
    let price = whole(d.get("lastprice").or_else(|| d.get("close")))
        .ok_or_else(|| IdxError::ParseError("stockbit order book: no last price".to_string()))?;
    let levels = |key: &str| -> Vec<BookLevel> {
        d.get(key)
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        Some(BookLevel {
                            price: whole(row.get("price")).filter(|p| *p > 0)?,
                            shares: count(row.get("volume")).unwrap_or(0),
                            orders: count(row.get("que_num")).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let listing = d.get("status").and_then(Value::as_str).unwrap_or_default();
    let volume = count(d.get("volume"));
    let session_note = if !listing.is_empty() && !listing.eq_ignore_ascii_case("active") {
        Some(format!(
            "Stockbit lists the stock as {}",
            listing.to_ascii_lowercase()
        ))
    } else if volume == Some(0) {
        Some("no trades this session".to_string())
    } else {
        None
    };
    // Stockbit flags whether `fbuy`/`fsell`/`fnet` are current. While the
    // flag was off (2026-11-03, in session) they were the previous session's
    // figures (equal to IPOT's for 2026-11-02 for BBCA, BBRI and TLKM), so
    // they are dropped rather than shown beside today's prices.
    let foreign_current = d.get("has_foreign_bs").and_then(Value::as_bool) == Some(true);
    let foreign = |key: &str| foreign_current.then(|| whole(d.get(key))).flatten();
    Ok(OrderBook {
        symbol: resolved.to_string(),
        name: d.get("name").and_then(Value::as_str).map(str::to_string),
        price,
        change: whole(d.get("change")),
        change_pct: d.get("percentage_change").and_then(number),
        prev_close: whole(d.get("previous")),
        open: whole(d.get("open")),
        high: whole(d.get("high")),
        low: whole(d.get("low")),
        volume,
        value: count(d.get("value")),
        frequency: count(d.get("frequency")),
        foreign_buy: foreign("fbuy"),
        foreign_sell: foreign("fsell"),
        foreign_net: foreign("fnet"),
        bids: levels("bid"),
        offers: levels("offer"),
        market_status,
        session_note,
    })
}

/// A quote from the order book; bid/offer only while the market is open.
/// Its `market_status` is the public page's vocabulary, `open` or `close`,
/// so a quote reads the same with or without a login; the order book keeps
/// Stockbit's state verbatim.
pub fn quote_from_book(book: &OrderBook) -> Quote {
    let open = book.market_status.as_deref().is_some_and(is_trading);
    let best = |levels: &[BookLevel]| open.then(|| levels.first().map(|l| l.price)).flatten();
    Quote {
        symbol: book.symbol.clone(),
        price: book.price,
        change: book.change,
        change_pct: book.change_pct,
        volume: book.volume,
        market_cap: None,
        week52_high: None,
        week52_low: None,
        week52_position: None,
        range_signal: None,
        prev_close: book.prev_close,
        avg_volume: None,
        as_of: None,
        behind_session: false,
        bid: best(&book.bids),
        offer: best(&book.offers),
        market_status: book
            .market_status
            .as_ref()
            .map(|_| if open { "open" } else { "close" }.to_string()),
        session_note: book.session_note.clone(),
    }
}

pub fn parse_market_status(body: &str) -> Option<String> {
    data(body, "market session")
        .ok()?
        .pointer("/detail/regular/state_name")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The chart window holding `period` of daily candles: Stockbit picks the
/// bar size from the window (`1w` is hourly, `3y`/`5y` weekly), so only the
/// daily windows qualify.
pub fn history_window(period: &Period, interval: &Interval) -> Option<&'static str> {
    match (interval, period) {
        (Interval::Day, Period::FiveDays | Period::OneMonth) => Some("1m"),
        (Interval::Day, Period::ThreeMonths) => Some("3m"),
        (Interval::Day, Period::SixMonths | Period::OneYear) => Some("1y"),
        _ => None,
    }
}

pub fn unsupported_window(period: &Period, interval: &Interval) -> IdxError {
    IdxError::Unsupported(format!(
        "Stockbit history covers 5d to 1y of daily candles, not {} at {}; use \
         --history-provider yahoo",
        period.as_str(),
        interval.as_str()
    ))
}

/// The candles of `period` from a daily chart response.
pub fn daily_for_period(body: &str, period: &Period) -> Result<Vec<Ohlc>, IdxError> {
    Ok(trim_to_period(parse_daily(body)?, period))
}

/// Daily candles, oldest first. Volume arrives in lots and is returned in
/// shares, as from the other providers.
pub fn parse_daily(body: &str) -> Result<Vec<Ohlc>, IdxError> {
    let d = data(body, "chart")?;
    let rows = d
        .get("prices")
        .and_then(Value::as_array)
        .ok_or_else(|| IdxError::ParseError("stockbit chart: no prices".to_string()))?;
    let mut bars = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(close) = whole(row.get("value")) else {
            continue;
        };
        let Some(date) = row
            .get("formatted_date")
            .and_then(Value::as_str)
            .and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
        else {
            return Err(IdxError::ParseError(format!(
                "stockbit chart: row without a daily date: {row}"
            )));
        };
        bars.push(Ohlc {
            date,
            open: whole(row.get("open")).unwrap_or(close),
            high: whole(row.get("high")).unwrap_or(close),
            low: whole(row.get("low")).unwrap_or(close),
            close,
            volume: count(row.get("volume")).unwrap_or(0).saturating_mul(100),
        });
    }
    bars.sort_by_key(|b| b.date);
    Ok(bars)
}

/// The candles within `period` of the latest one (5 sessions for `5d`).
fn trim_to_period(mut bars: Vec<Ohlc>, period: &Period) -> Vec<Ohlc> {
    let Some(last) = bars.last().map(|b| b.date) else {
        return bars;
    };
    let months = match period {
        Period::FiveDays => {
            let start = bars.len().saturating_sub(5);
            return bars.split_off(start);
        }
        Period::OneMonth => 1,
        Period::ThreeMonths => 3,
        Period::SixMonths => 6,
        _ => 12,
    };
    match last.checked_sub_months(Months::new(months)) {
        Some(start) => bars.into_iter().filter(|b| b.date > start).collect(),
        None => bars,
    }
}

/// A 403 from the CDN in front of the session API: a block, not a refused
/// session, so nothing is retried.
pub(crate) fn blocked() -> IdxError {
    IdxError::UpstreamBlocked(
        "exodus.stockbit.com refused this client (HTTP 403 from its CDN); idx does not \
         retry a block. Try again later or from another network"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/stockbit_exodus_{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn order_book_carries_depth_in_shares_and_foreign_flow() {
        let book = parse_orderbook("BBCA.JK", &fixture("orderbook_bbca.json"), None).unwrap();
        assert_eq!(
            (book.price, book.prev_close, book.change),
            (5175, Some(6075), Some(-900))
        );
        assert_eq!(
            (book.open, book.high, book.low),
            (Some(5825), Some(5825), Some(5175))
        );
        assert_eq!(book.foreign_net, Some(-2_329_493_087_500));
        // BBCA closed at its lower limit: no bids, offers best price first.
        assert!(book.bids.is_empty());
        assert_eq!(
            book.offers[0],
            BookLevel {
                price: 5175,
                shares: 264_462_500,
                orders: 3284
            }
        );
        assert!(book.offers.windows(2).all(|w| w[0].price < w[1].price));
        assert_eq!(book.session_note, None);
    }

    #[test]
    fn quote_shows_the_best_levels_only_while_the_market_is_open() {
        let trading = parse_market_status(&fixture("market_session_trading.json"));
        let book = |state: Option<String>| {
            parse_orderbook("BBCA.JK", &fixture("orderbook_bbca_in_session.json"), state).unwrap()
        };
        let open = quote_from_book(&book(trading));
        assert_eq!((open.bid, open.offer), (Some(4850), Some(4860)));
        assert_eq!(open.price, 4860);
        assert_eq!(open.market_status.as_deref(), Some("open"));
        for state in ["market_session_break.json", "market_session.json"] {
            let shut = quote_from_book(&book(parse_market_status(&fixture(state))));
            assert_eq!((shut.bid, shut.offer), (None, None), "{state}");
            assert_eq!(shut.market_status.as_deref(), Some("close"), "{state}");
        }
        let unknown = quote_from_book(&book(None));
        assert_eq!((unknown.bid, unknown.market_status), (None, None));
    }

    #[test]
    fn market_status_is_read_verbatim() {
        for (name, state) in [
            ("market_session.json", "STATE_NAME_MARKET_CLOSED"),
            ("market_session_break.json", "STATE_NAME_BREAK"),
            ("market_session_trading.json", "STATE_NAME_SESSION_2"),
        ] {
            assert_eq!(parse_market_status(&fixture(name)).as_deref(), Some(state));
        }
    }

    #[test]
    fn only_numbered_sessions_are_trading() {
        assert!(is_trading("STATE_NAME_SESSION_1"));
        assert!(is_trading("STATE_NAME_SESSION_2"));
        for state in [
            "STATE_NAME_BREAK",
            "STATE_NAME_MARKET_CLOSED",
            "STATE_NAME_MARKET_OPEN",
            "STATE_NAME_SESSION_",
            "STATE_NAME_SESSION_PRE_OPENING",
            "",
        ] {
            assert!(!is_trading(state), "{state}");
        }
    }

    #[test]
    fn foreign_flow_only_when_stockbit_marks_it_current() {
        // In session (has_foreign_bs false) the figures were 2026-11-02's.
        let live =
            parse_orderbook("BBCA.JK", &fixture("orderbook_bbca_in_session.json"), None).unwrap();
        assert_eq!(
            (live.foreign_buy, live.foreign_sell, live.foreign_net),
            (None, None, None)
        );
        assert_eq!(live.volume, Some(134_744_100));
        let done = parse_orderbook("BBCA.JK", &fixture("orderbook_bbca.json"), None).unwrap();
        assert_eq!(done.foreign_buy, Some(1_616_461_977_500));
    }

    #[test]
    fn errors_map_to_their_codes() {
        let reply = |status, body: &str| Reply {
            status,
            body: body.to_string(),
        };
        assert!(matches!(
            classify(reply(404, &fixture("orderbook_zzzz.json")), "ZZZZ.JK"),
            Err(IdxError::SymbolNotFound(s)) if s == "ZZZZ.JK"
        ));
        assert!(reply(401, &fixture("unauthorized.json")).rejected());
        assert!(reply(403, &fixture("unauthorized.json")).rejected());
        let blocked = reply(403, "<HTML>Request blocked.</HTML>");
        assert!(!blocked.rejected());
        assert!(matches!(
            classify(blocked, "BBCA.JK"),
            Err(IdxError::UpstreamBlocked(_))
        ));
        assert!(matches!(
            classify(reply(429, ""), "x"),
            Err(IdxError::RateLimited)
        ));
    }

    #[test]
    fn daily_candles_convert_lots_to_shares_and_trim_to_the_period() {
        let year = parse_daily(&fixture("chart_candle_bbca_1y.json")).unwrap();
        assert_eq!(year.len(), 240);
        assert!(year.windows(2).all(|w| w[0].date < w[1].date));
        let month = parse_daily(&fixture("chart_candle_bbca_1m.json")).unwrap();
        let first = &month[0];
        assert_eq!(first.date.to_string(), "2026-09-16");
        assert_eq!(
            (first.open, first.high, first.low, first.close),
            (6350, 6425, 6325, 6325)
        );
        assert_eq!(first.volume, 72_157_400);

        let six = trim_to_period(year.clone(), &Period::SixMonths);
        assert_eq!(six.first().unwrap().date.to_string(), "2026-04-17");
        assert_eq!(trim_to_period(month, &Period::FiveDays).len(), 5);
    }

    #[test]
    fn only_daily_windows_map_to_a_chart() {
        assert_eq!(history_window(&Period::OneYear, &Interval::Day), Some("1y"));
        assert_eq!(history_window(&Period::TwoYears, &Interval::Day), None);
        assert_eq!(history_window(&Period::OneYear, &Interval::Week), None);
        assert_eq!(history_window(&Period::OneDay, &Interval::Day), None);
    }
}
