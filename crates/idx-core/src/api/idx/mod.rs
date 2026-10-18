//! IDX's own site: the daily trading summary of every listed stock.
//!
//! `GET https://www.idx.co.id/primary/TradingSummary/GetStockSummary`
//! `?length=9999&start=0&date=YYYYMMDD` answers JSON with one row per stock:
//! prices, volume and value, foreign buy and sell volume, and non-regular
//! (negotiated and cash) volume and value. A day without trading has no
//! rows. Cloudflare in front of the site rejects some TLS fingerprints, so
//! the request goes through curl-impersonate's profile fallback.

mod parse;

use chrono::NaiveDate;
pub use parse::parse_stock_summary;

use super::TradingSummaryProvider;
use crate::curl_impersonate;
use crate::error::IdxError;
use crate::flow::types::StockDay;

const STOCK_SUMMARY_URL: &str = "https://www.idx.co.id/primary/TradingSummary/GetStockSummary";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.4 Safari/605.1.15";
/// One session's summary is about 650 KB; it must not hold a command or an
/// MCP call for long.
const MAX_TIME_SECS: &str = "30";

pub fn stock_summary_url(date: NaiveDate) -> String {
    format!(
        "{STOCK_SUMMARY_URL}?length=9999&start=0&date={}",
        date.format("%Y%m%d")
    )
}

/// A Cloudflare challenge page is not the JSON asked for: try the next
/// profile.
fn accept_json(body: &[u8]) -> Result<(), IdxError> {
    match body.iter().find(|byte| !byte.is_ascii_whitespace()) {
        Some(b'{') => Ok(()),
        _ => Err(IdxError::Http(
            "IDX stock summary: answered a challenge page instead of JSON (HTTP 403)".to_string(),
        )),
    }
}

pub struct IdxSiteProvider;

impl TradingSummaryProvider for IdxSiteProvider {
    fn stock_summary(&self, date: NaiveDate) -> Result<Vec<StockDay>, IdxError> {
        let args: Vec<String> = [
            "--silent",
            "--show-error",
            "--location",
            "--fail",
            "--compressed",
            "--max-time",
            MAX_TIME_SECS,
            "--header",
            &format!("User-Agent: {USER_AGENT}"),
            "--header",
            "Accept: application/json,text/plain,*/*",
            "--header",
            "Referer: https://www.idx.co.id/id/data-pasar/ringkasan-perdagangan/ringkasan-saham/",
            &stock_summary_url(date),
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
        let output =
            curl_impersonate::run_owned_with_fallback("IDX stock summary", &args, &accept_json)?;
        let body = String::from_utf8(output.stdout)
            .map_err(|e| IdxError::Http(format!("IDX stock summary: not UTF-8: {e}")))?;
        parse_stock_summary(&body, date)
    }
}
