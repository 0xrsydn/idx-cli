//! Pure parsing of a Stockbit symbol page: the `__NEXT_DATA__` JSON that
//! Next.js renders into `https://stockbit.com/symbol/{CODE}`. No I/O.
//!
//! Shapes (recorded 2026-10-22 after the close, see
//! `tests/fixtures/stockbit_symbol_*.html`):
//! - `props.pageProps.company`: the quote. Numbers are mostly strings
//!   (`price: "5175"`, an index `"5387.90478515625"`, `change: "+59.54"`),
//!   `percentage` is a JSON number, `orderbook.{bid,offer}.price` is
//!   `"5175.000000"` or `"N/A"`, `market_hour.status` is e.g. `close`, and
//!   `status` is `STATUS_ACTIVE`, `STATUS_SUSPENDED` or `STATUS_DELISTED`.
//!   `date`/`time` (`"22 Oct 2026"`, `"Thu 16:14"`) are the last trade only
//!   when the stock traded: a suspended one carries today's date.
//! - `props.pageProps.chart.prices[]`: one row per minute,
//!   `formatted_date: "2026-10-22 16:14:00"` (WIB, no offset) and
//!   `value: "5175"`. The first row is a synthetic anchor at the previous
//!   close with an empty `formatted_date`. Per-minute OHLC and volume are
//!   empty strings, so only the price is kept.
//! - An unknown code renders the 404 page: `page: "/404"`, empty pageProps.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::Deserialize;
use serde_json::Value;

use crate::api::idx_offset;
use crate::api::types::{Intraday, IntradayPoint, Quote};
use crate::error::IdxError;

/// Stockbit's market status while continuous trading runs. Bid and offer are
/// reported only then: outside it they are the closing book.
const MARKET_OPEN: &str = "open";

#[derive(Debug, Deserialize)]
struct NextData {
    page: String,
    props: Props,
}

#[derive(Debug, Deserialize)]
struct Props {
    #[serde(rename = "pageProps", default)]
    page_props: PageProps,
}

#[derive(Debug, Default, Deserialize)]
struct PageProps {
    company: Option<RawCompany>,
    chart: Option<RawChart>,
}

#[derive(Debug, Deserialize)]
struct RawCompany {
    price: Value,
    #[serde(default)]
    previous: Value,
    #[serde(default)]
    change: Value,
    #[serde(default)]
    percentage: Value,
    #[serde(default)]
    volume: Value,
    date: Option<String>,
    time: Option<String>,
    status: Option<String>,
    orderbook: Option<RawOrderbook>,
    market_hour: Option<RawMarketHour>,
}

#[derive(Debug, Deserialize)]
struct RawOrderbook {
    bid: Option<RawLevel>,
    offer: Option<RawLevel>,
}

#[derive(Debug, Deserialize)]
struct RawLevel {
    #[serde(default)]
    price: Value,
}

#[derive(Debug, Deserialize)]
struct RawMarketHour {
    status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawChart {
    #[serde(default)]
    prices: Vec<RawPoint>,
}

#[derive(Debug, Deserialize)]
struct RawPoint {
    #[serde(default)]
    formatted_date: String,
    #[serde(default)]
    value: Value,
}

/// Stockbit's code for a resolved symbol: `BBCA.JK` -> `BBCA`, and Yahoo's
/// index tickers to Stockbit's (`^JKSE` -> `IHSG`, `^JKLQ45` -> `LQ45`).
pub fn stockbit_code(resolved: &str) -> Result<String, IdxError> {
    let unsupported = || {
        IdxError::Unsupported(format!(
            "{resolved}: Stockbit covers IDX (.JK) symbols only"
        ))
    };
    let code = resolved.strip_suffix(".JK").ok_or_else(unsupported)?;
    let code = match code.strip_prefix('^') {
        Some("JKSE") => "IHSG",
        Some("JKLQ45") => "LQ45",
        Some(_) => return Err(unsupported()),
        None => code,
    };
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(unsupported());
    }
    Ok(code.to_string())
}

/// The error for a non-success HTTP status from stockbit.com. A 403 is
/// CloudFront's static block page (it refuses some user agents outright),
/// not a challenge, so nothing is retried.
pub fn status_error(status: u16, resolved: &str) -> IdxError {
    match status {
        403 => IdxError::UpstreamBlocked(
            "stockbit.com refused this client (HTTP 403 from its CDN); idx does not retry a \
             block. Try again later or from another network; `--provider msn` serves quotes \
             without Stockbit"
                .to_string(),
        ),
        404 => IdxError::SymbolNotFound(resolved.to_string()),
        429 => IdxError::RateLimited,
        _ => IdxError::Http(format!("stockbit: HTTP {status}")),
    }
}

/// The quote and today's per-minute prices from a symbol page, stamped with
/// `resolved` (e.g. `BBCA.JK`).
pub fn parse_symbol_page(resolved: &str, html: &str) -> Result<Intraday, IdxError> {
    let data: NextData = serde_json::from_str(next_data(html)?)
        .map_err(|e| layout_error(format!("__NEXT_DATA__ is not the expected JSON: {e}")))?;
    if data.page == "/404" {
        return Err(IdxError::SymbolNotFound(resolved.to_string()));
    }
    let company = data
        .props
        .page_props
        .company
        .ok_or_else(|| layout_error("no company in the page".to_string()))?;
    let points = parse_points(&data.props.page_props.chart.unwrap_or_default())?;
    let quote = parse_quote(resolved, &company)?;
    Ok(Intraday { quote, points })
}

fn layout_error(detail: String) -> IdxError {
    IdxError::ParseError(format!(
        "stockbit symbol page: {detail} (the page layout changed?)"
    ))
}

/// The JSON inside `<script id="__NEXT_DATA__" ...>...</script>`.
fn next_data(html: &str) -> Result<&str, IdxError> {
    let missing = || layout_error("no __NEXT_DATA__ script".to_string());
    let tag = html.find("id=\"__NEXT_DATA__\"").ok_or_else(missing)?;
    let start = tag + html[tag..].find('>').ok_or_else(missing)? + 1;
    let len = html[start..].find("</script>").ok_or_else(missing)?;
    Ok(&html[start..start + len])
}

fn parse_quote(resolved: &str, company: &RawCompany) -> Result<Quote, IdxError> {
    let price = number(&company.price)
        .ok_or_else(|| layout_error(format!("price {} is not a number", company.price)))?;
    let price = price.round() as i64;
    let prev_close = number(&company.previous).map(|p| p.round() as i64);
    let change = number(&company.change)
        .map(|c| c.round() as i64)
        .or_else(|| prev_close.map(|prev| price - prev));
    let volume = count(&company.volume);
    let listing = company.status.as_deref().unwrap_or_default();
    let market_status = company
        .market_hour
        .as_ref()
        .and_then(|m| m.status.clone())
        .filter(|s| !s.is_empty());
    let open = market_status.as_deref() == Some(MARKET_OPEN);
    let level = |side: Option<&RawLevel>| {
        side.and_then(|l| number(&l.price))
            .filter(|p| *p > 0.0)
            .map(|p| p.round() as i64)
    };
    let book = company.orderbook.as_ref();
    let (bid, offer) = if open {
        (
            level(book.and_then(|b| b.bid.as_ref())),
            level(book.and_then(|b| b.offer.as_ref())),
        )
    } else {
        (None, None)
    };
    let traded = volume.is_some_and(|v| v > 0);
    let as_of = if listing == "STATUS_ACTIVE" && traded {
        last_trade(company.date.as_deref(), company.time.as_deref())
    } else {
        None
    };
    // Facts about this symbol only; the market's state is `market_status`.
    // An untraded stock can still have a flat chart at the previous close.
    let session_note = match listing {
        "STATUS_SUSPENDED" => Some("Stockbit lists the stock as suspended".to_string()),
        "STATUS_DELISTED" => Some("Stockbit lists the stock as delisted".to_string()),
        _ if volume == Some(0) => Some("no trades this session".to_string()),
        _ => None,
    };
    Ok(Quote {
        symbol: resolved.to_string(),
        price,
        change,
        change_pct: number(&company.percentage),
        volume,
        market_cap: None,
        week52_high: None,
        week52_low: None,
        week52_position: None,
        range_signal: None,
        prev_close,
        avg_volume: None,
        as_of,
        behind_session: false,
        bid,
        offer,
        market_status,
        session_note,
    })
}

/// Today's prices, oldest first, one per minute. The anchor row (no
/// timestamp) is dropped; any other row that does not parse means the
/// layout changed.
fn parse_points(chart: &RawChart) -> Result<Vec<IntradayPoint>, IdxError> {
    let mut points = Vec::with_capacity(chart.prices.len());
    for row in &chart.prices {
        if row.formatted_date.is_empty() {
            continue;
        }
        let bad = || layout_error(format!("chart row {:?} {}", row.formatted_date, row.value));
        let local = NaiveDateTime::parse_from_str(&row.formatted_date, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| bad())?;
        let time = wib(local).ok_or_else(bad)?;
        let price = number(&row.value).ok_or_else(bad)?;
        points.push(IntradayPoint { time, price });
    }
    points.sort_by_key(|p| p.time);
    // A repeated minute keeps its last row.
    points.reverse();
    points.dedup_by_key(|p| p.time);
    points.reverse();
    Ok(points)
}

/// `"22 Oct 2026"` and `"Thu 16:14"` as a WIB time.
fn last_trade(date: Option<&str>, time: Option<&str>) -> Option<DateTime<FixedOffset>> {
    let date = NaiveDate::parse_from_str(date?.trim(), "%d %b %Y").ok()?;
    let clock = time?.trim().rsplit(' ').next()?;
    let clock = NaiveTime::parse_from_str(clock, "%H:%M").ok()?;
    wib(date.and_time(clock))
}

fn wib(local: NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    idx_offset().from_local_datetime(&local).single()
}

/// A JSON number or numeric string (`"+59.54"`, `"5175.000000"`); `"N/A"`,
/// `"NA"` and empty strings are absent.
pub(super) fn number(value: &Value) -> Option<f64> {
    let n = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

fn count(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// The recorded BBCA page with Stockbit's market status set to `status`.
    fn bbca_with_status(status: &str) -> String {
        let page = fixture("stockbit_symbol_bbca.html");
        assert!(page.contains(r#""status":"close""#));
        page.replace(r#""status":"close""#, &format!(r#""status":"{status}""#))
    }

    #[test]
    fn closed_session_keeps_the_series_and_hides_the_closing_book() {
        let day = parse_symbol_page("BBCA.JK", &fixture("stockbit_symbol_bbca.html")).unwrap();
        let q = &day.quote;
        assert_eq!(
            (q.price, q.prev_close, q.change),
            (5175, Some(5275), Some(-100))
        );
        assert_eq!(q.change_pct, Some(-1.9));
        assert_eq!(q.volume, Some(286_928_500));
        assert_eq!(q.market_status.as_deref(), Some("close"));
        assert_eq!((q.bid, q.offer), (None, None));
        assert_eq!(q.session_note, None);
        assert_eq!(q.as_of.unwrap().to_rfc3339(), "2026-10-22T16:14:00+07:00");

        // 336 rows less the anchor at the previous close.
        assert_eq!(day.points.len(), 335);
        let first = &day.points[0];
        assert_eq!(first.time.to_rfc3339(), "2026-10-22T08:58:00+07:00");
        assert_eq!(first.price, 5125.0);
        let last = day.points.last().unwrap();
        assert_eq!(last.time.to_rfc3339(), "2026-10-22T16:14:00+07:00");
        assert_eq!(last.price, 5175.0);
        assert!(day.points.windows(2).all(|w| w[0].time < w[1].time));
    }

    #[test]
    fn open_market_reports_best_bid_and_offer() {
        let day = parse_symbol_page("BBCA.JK", &bbca_with_status(MARKET_OPEN)).unwrap();
        assert_eq!((day.quote.bid, day.quote.offer), (Some(5175), Some(5200)));
        assert_eq!(day.quote.market_status.as_deref(), Some(MARKET_OPEN));
    }

    #[test]
    fn any_other_market_status_hides_bid_and_offer() {
        for status in ["pre_open", "break", "OPEN", ""] {
            let day = parse_symbol_page("BBCA.JK", &bbca_with_status(status)).unwrap();
            assert_eq!((day.quote.bid, day.quote.offer), (None, None), "{status:?}");
        }
    }

    #[test]
    fn index_keeps_decimals_in_the_series_and_rounds_the_quote() {
        let day = parse_symbol_page("^JKSE.JK", &fixture("stockbit_symbol_ihsg.html")).unwrap();
        let q = &day.quote;
        assert_eq!(
            (q.price, q.prev_close, q.change),
            (5388, Some(5328), Some(60))
        );
        // An index has no book even while open.
        assert_eq!((q.bid, q.offer), (None, None));
        assert_eq!(day.points.last().unwrap().price, 5387.90);
    }

    #[test]
    fn suspended_and_delisted_stocks_are_empty_sessions_not_errors() {
        for (file, note) in [
            (
                "stockbit_symbol_maba.html",
                "Stockbit lists the stock as suspended",
            ),
            (
                "stockbit_symbol_myrx.html",
                "Stockbit lists the stock as delisted",
            ),
        ] {
            let day = parse_symbol_page("X.JK", &fixture(file)).unwrap();
            assert!(day.points.is_empty(), "{file}");
            assert_eq!(day.quote.session_note.as_deref(), Some(note), "{file}");
            // The page's date/time is not a trade time when nothing traded.
            assert_eq!(day.quote.as_of, None, "{file}");
        }
    }

    /// HOME and BIMA on 2026-10-22: active, a flat chart at the previous
    /// close, volume 0. Without a note they read as a traded flat session.
    #[test]
    fn an_untraded_active_stock_says_so_even_with_a_flat_chart() {
        let page = fixture("stockbit_symbol_bbca.html");
        assert!(page.contains(r#""volume":"286928500""#));
        let page = page.replace(r#""volume":"286928500""#, r#""volume":"0""#);
        let day = parse_symbol_page("BBCA.JK", &page).unwrap();
        assert!(!day.points.is_empty());
        assert_eq!(
            day.quote.session_note.as_deref(),
            Some("no trades this session")
        );
        assert_eq!(day.quote.as_of, None);
    }

    #[test]
    fn unknown_code_is_symbol_not_found() {
        let err = parse_symbol_page("ZZZZ.JK", &fixture("stockbit_symbol_zzzz_404.html"));
        assert!(matches!(err, Err(IdxError::SymbolNotFound(s)) if s == "ZZZZ.JK"));
    }

    #[test]
    fn a_page_without_next_data_is_a_layout_change() {
        let err = parse_symbol_page("BBCA.JK", &fixture("stockbit_blocked_403.html"));
        assert!(matches!(err, Err(IdxError::ParseError(_))), "{err:?}");
    }

    #[test]
    fn http_403_is_upstream_blocked_and_404_is_not_found() {
        assert!(matches!(
            status_error(403, "BBCA.JK"),
            IdxError::UpstreamBlocked(_)
        ));
        assert!(
            matches!(status_error(404, "BBCA.JK"), IdxError::SymbolNotFound(s) if s == "BBCA.JK")
        );
        assert!(matches!(
            status_error(429, "BBCA.JK"),
            IdxError::RateLimited
        ));
        assert!(matches!(status_error(502, "BBCA.JK"), IdxError::Http(_)));
    }

    #[test]
    fn codes_map_idx_symbols_and_index_tickers() {
        assert_eq!(stockbit_code("BBCA.JK").unwrap(), "BBCA");
        assert_eq!(stockbit_code("^JKSE.JK").unwrap(), "IHSG");
        assert_eq!(stockbit_code("^JKLQ45.JK").unwrap(), "LQ45");
        for other in ["AAPL.US", "^GSPC.JK", "BRK-B.JK", ".JK"] {
            assert!(
                matches!(stockbit_code(other), Err(IdxError::Unsupported(_))),
                "{other}"
            );
        }
    }
}
