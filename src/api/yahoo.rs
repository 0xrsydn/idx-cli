use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;

use crate::api::MarketDataProvider;
use crate::api::types::{Fundamentals, Interval, Ohlc, Period, Quote};
use crate::error::IdxError;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const BASE_URL: &str = "https://query2.finance.yahoo.com";
const COOKIE_FETCH_URL: &str = "https://fc.yahoo.com";
const CRUMB_FETCH_URL: &str = "https://query1.finance.yahoo.com/v1/test/getcrumb";

pub struct YahooProvider {
    agent: ureq::Agent,
    verbose: bool,
    crumb: Mutex<Option<String>>,
}

impl YahooProvider {
    pub fn new(verbose: bool) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .build()
            .into();

        Self {
            agent,
            verbose,
            crumb: Mutex::new(None),
        }
    }

    fn chart_url(symbol: &str, period: &Period, interval: &Interval) -> String {
        format!(
            "{BASE_URL}/v8/finance/chart/{symbol}?range={}&interval={}",
            period.as_str(),
            interval.as_str()
        )
    }

    fn quote_summary_url(symbol: &str, crumb: &str) -> String {
        format!(
            "{BASE_URL}/v10/finance/quoteSummary/{symbol}?modules=defaultKeyStatistics,financialData,incomeStatementHistory&crumb={crumb}"
        )
    }

    fn cookie_jar_path() -> PathBuf {
        PathBuf::from(format!("/tmp/idx_yf_{}.txt", std::process::id()))
    }

    fn parse_crumb_body(raw: &str) -> Result<String, IdxError> {
        let crumb = raw.trim();
        if crumb.is_empty() {
            return Err(IdxError::Http("received empty Yahoo crumb".to_string()));
        }

        let normalized = crumb.to_ascii_lowercase();
        if normalized.contains("<html>") || normalized.contains("<!doctype html") {
            return Err(IdxError::Http(
                "received HTML instead of a Yahoo crumb".to_string(),
            ));
        }
        if normalized.contains("too many requests") {
            return Err(IdxError::Http(
                "Yahoo crumb request was rate limited".to_string(),
            ));
        }

        Ok(crumb.to_string())
    }

    fn cookie_header_from_jar(path: &Path) -> Result<String, IdxError> {
        let jar = std::fs::read_to_string(path).map_err(|e| {
            IdxError::Http(format!(
                "failed to read Yahoo cookie jar {}: {e}",
                path.display()
            ))
        })?;

        let cookies: Vec<String> = jar
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }

                let candidate = trimmed.strip_prefix("#HttpOnly_").unwrap_or(trimmed);
                if candidate.starts_with('#') {
                    return None;
                }

                let fields: Vec<_> = candidate.split('\t').collect();
                if fields.len() < 7 {
                    return None;
                }

                let name = fields[5].trim();
                let value = fields[6].trim();
                if name.is_empty() {
                    return None;
                }

                Some(format!("{name}={value}"))
            })
            .collect();

        if cookies.is_empty() {
            return Err(IdxError::Http(format!(
                "Yahoo cookie jar {} did not contain any cookies",
                path.display()
            )));
        }

        Ok(cookies.join("; "))
    }

    fn chrome_curl_binary() -> Option<&'static str> {
        // curl-impersonate-chrome ships per-version binaries (curl_chrome131 etc).
        // Try latest versions first; no --impersonate flag needed — the binary IS the impersonation.
        const CANDIDATES: &[&str] = &[
            "curl_chrome136",
            "curl_chrome133a",
            "curl_chrome131",
            "curl_chrome124",
            "curl_chrome120",
            "curl_chrome116",
        ];
        CANDIDATES
            .iter()
            .copied()
            .find(|bin| Command::new(bin).arg("--version").output().is_ok())
    }

    fn run_curl(stage: &str, binary: &str, args: &[&str]) -> Result<Output, IdxError> {
        let output = Command::new(binary).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                return IdxError::Http(format!(
                    "curl-impersonate binary '{binary}' not found; install nixpkgs#curl-impersonate-chrome"
                ));
            }
            IdxError::Http(format!("failed to run {binary} for Yahoo {stage}: {e}"))
        })?;

        if output.status.success() {
            return Ok(output);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        Err(IdxError::Http(format!(
            "Yahoo {stage} {binary} failed (status {}): {}",
            output.status,
            if detail.is_empty() {
                "no output"
            } else {
                detail
            }
        )))
    }

    fn fetch_crumb_via_curl(&self) -> Result<String, IdxError> {
        let binary = Self::chrome_curl_binary().ok_or_else(|| {
            IdxError::Http(
                "no curl_chrome* binary found; install nixpkgs#curl-impersonate-chrome".to_string(),
            )
        })?;

        let cookie_jar = Self::cookie_jar_path();
        let cookie_jar_str = cookie_jar.to_str().ok_or_else(|| {
            IdxError::Io(format!(
                "failed to encode Yahoo cookie jar path {}",
                cookie_jar.display()
            ))
        })?;

        // Step 1: fetch fc.yahoo.com to set A3 cookie (returns 404 but writes cookie jar)
        // We allow non-zero exit here since 404 still writes the cookie
        let _ = Command::new(binary)
            .args([
                "--silent",
                "--cookie-jar",
                cookie_jar_str,
                COOKIE_FETCH_URL,
                "--output",
                "/dev/null",
            ])
            .output();

        // Step 2: fetch crumb with cookie jar (Chrome TLS fingerprint + A3 cookie)
        let output = Self::run_curl(
            "crumb fetch",
            binary,
            &["--silent", "--cookie", cookie_jar_str, CRUMB_FETCH_URL],
        )?;

        let body = String::from_utf8_lossy(&output.stdout);
        Self::parse_crumb_body(&body)
    }

    fn get_or_init_crumb(&self) -> Result<String, IdxError> {
        let mut guard = self
            .crumb
            .lock()
            .map_err(|e| IdxError::Io(format!("crumb lock poisoned: {e}")))?;

        if let Some(crumb) = guard.as_ref() {
            return Ok(crumb.clone());
        }

        let crumb = self.fetch_crumb_via_curl()?;
        *guard = Some(crumb.clone());
        Ok(crumb)
    }

    fn clear_crumb(&self) -> Result<(), IdxError> {
        let mut guard = self
            .crumb
            .lock()
            .map_err(|e| IdxError::Io(format!("crumb lock poisoned: {e}")))?;
        *guard = None;
        Ok(())
    }

    fn fetch_chart(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<ChartResponse, IdxError> {
        let mut wait = Duration::from_millis(250);
        for attempt in 0..3 {
            let url = Self::chart_url(symbol, period, interval);
            let response = self.agent.get(&url).header("User-Agent", USER_AGENT).call();
            match response {
                Ok(ok) => {
                    let chart = ok
                        .into_body()
                        .read_json::<ChartResponse>()
                        .map_err(|e| IdxError::ParseError(e.to_string()))?;
                    if let Some(err) = chart.chart.error.as_ref() {
                        return Err(map_yahoo_error(symbol, "chart", err));
                    }
                    return Ok(chart);
                }
                Err(ureq::Error::StatusCode(429)) => {
                    if attempt < 2 {
                        std::thread::sleep(wait + jitter());
                        wait *= 2;
                    }
                }
                Err(ureq::Error::StatusCode(404)) => {
                    return Err(IdxError::SymbolNotFound(symbol.to_string()));
                }
                Err(e) => return Err(IdxError::Http(e.to_string())),
            }
        }
        Err(IdxError::RateLimited)
    }

    fn fetch_quote_summary(&self, symbol: &str) -> Result<QuoteSummaryResponse, IdxError> {
        for auth_attempt in 0..2 {
            let crumb = self.get_or_init_crumb()?;
            // Read the A3 cookie written during crumb fetch and pass it to quoteSummary
            let cookie_header =
                Self::cookie_header_from_jar(&Self::cookie_jar_path()).unwrap_or_default();
            let url = Self::quote_summary_url(symbol, &crumb);
            let mut wait = Duration::from_millis(250);

            for attempt in 0..3 {
                let mut req = self.agent.get(&url).header("User-Agent", USER_AGENT);
                if !cookie_header.is_empty() {
                    req = req.header("Cookie", &cookie_header);
                }
                let response = req.call();
                match response {
                    Ok(ok) => {
                        let quote_summary = ok
                            .into_body()
                            .read_json::<QuoteSummaryResponse>()
                            .map_err(|e| IdxError::ParseError(e.to_string()))?;
                        if let Some(err) = quote_summary.quote_summary.error.as_ref() {
                            return Err(map_yahoo_error(symbol, "quoteSummary", err));
                        }
                        return Ok(quote_summary);
                    }
                    Err(ureq::Error::StatusCode(401)) => {
                        if auth_attempt == 0 {
                            self.clear_crumb()?;
                            break;
                        }
                        return Err(IdxError::Http(
                            "yahoo quoteSummary returned unauthorized (401)".to_string(),
                        ));
                    }
                    Err(ureq::Error::StatusCode(429)) => {
                        if attempt < 2 {
                            std::thread::sleep(wait + jitter());
                            wait *= 2;
                        }
                    }
                    Err(ureq::Error::StatusCode(404)) => {
                        return Err(IdxError::SymbolNotFound(symbol.to_string()));
                    }
                    Err(e) => return Err(IdxError::Http(e.to_string())),
                }
            }
        }

        Err(IdxError::RateLimited)
    }
}

fn jitter() -> Duration {
    Duration::from_millis(fastrand::u64(0..100))
}

fn round_price(value: f64) -> i64 {
    value.round() as i64
}

// verbose behavior is configured on YahooProvider and threaded into history parsing.

fn map_yahoo_error(symbol: &str, endpoint: &str, err: &ChartError) -> IdxError {
    if err.code.eq_ignore_ascii_case("Not Found") {
        return IdxError::SymbolNotFound(symbol.to_string());
    }
    IdxError::Http(format!(
        "yahoo {endpoint} error {}: {}",
        err.code, err.description
    ))
}

impl MarketDataProvider for YahooProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let chart = self.fetch_chart(symbol, &Period::OneDay, &Interval::Day)?;
        parse_quote(symbol, &chart)
    }

    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        let quote_summary = self.fetch_quote_summary(symbol)?;
        parse_fundamentals(symbol, &quote_summary)
    }

    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        let chart = self.fetch_chart(symbol, period, interval)?;
        parse_history_with_verbose(&chart, self.verbose)
    }
}

pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_yahoo_error(symbol, "chart", err));
    }
    parse_quote(symbol, &chart)
}

fn parse_quote(symbol: &str, chart: &ChartResponse) -> Result<Quote, IdxError> {
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_yahoo_error(symbol, "chart", err));
    }

    let result = chart
        .chart
        .result
        .as_ref()
        .and_then(|r| r.first())
        .ok_or(IdxError::ProviderUnavailable)?;
    let meta = result.meta.as_ref().ok_or(IdxError::ProviderUnavailable)?;
    let raw_price = meta
        .regular_market_price
        .ok_or(IdxError::SymbolNotFound(symbol.to_string()))?;
    let raw_prev_close = meta.previous_close.or(meta.chart_previous_close);

    let price = round_price(raw_price);
    let prev_close = raw_prev_close.map(round_price);
    let change = prev_close.map_or(0, |p| price - p);
    let change_pct = raw_prev_close.map_or(0.0, |p| {
        if p != 0.0 {
            ((raw_price - p) / p) * 100.0
        } else {
            0.0
        }
    });

    let (week52_position, range_signal) = match (meta.fifty_two_week_low, meta.fifty_two_week_high)
    {
        (Some(low), Some(high)) if high > low => {
            let pos = (raw_price - low) / (high - low);
            let signal = if pos > 0.66 {
                "upper"
            } else if pos < 0.33 {
                "lower"
            } else {
                "middle"
            };
            (Some(pos), Some(signal.to_string()))
        }
        _ => (None, None),
    };

    Ok(Quote {
        symbol: meta.symbol.clone().unwrap_or_else(|| symbol.to_string()),
        price,
        change,
        change_pct,
        volume: meta.regular_market_volume.unwrap_or(0),
        market_cap: meta.market_cap,
        week52_high: meta.fifty_two_week_high.map(round_price),
        week52_low: meta.fifty_two_week_low.map(round_price),
        week52_position,
        range_signal,
        prev_close,
        avg_volume: meta.average_daily_volume_3month,
    })
}

pub(crate) fn parse_history_from_str(raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history_with_verbose(&chart, false)
}

pub(crate) fn parse_fundamentals_from_str(
    symbol: &str,
    raw: &str,
) -> Result<Fundamentals, IdxError> {
    let quote_summary: QuoteSummaryResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    if let Some(err) = quote_summary.quote_summary.error.as_ref() {
        return Err(map_yahoo_error(symbol, "quoteSummary", err));
    }
    parse_fundamentals(symbol, &quote_summary)
}

fn parse_history_with_verbose(chart: &ChartResponse, verbose: bool) -> Result<Vec<Ohlc>, IdxError> {
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_yahoo_error("unknown", "chart", err));
    }

    let result = chart
        .chart
        .result
        .as_ref()
        .and_then(|r| r.first())
        .ok_or(IdxError::ProviderUnavailable)?;
    let timestamps = result
        .timestamp
        .as_ref()
        .ok_or(IdxError::ProviderUnavailable)?;
    let quote = result
        .indicators
        .as_ref()
        .and_then(|i| i.quote.as_ref())
        .and_then(|q| q.first())
        .ok_or(IdxError::ProviderUnavailable)?;

    let mut out = Vec::new();
    let mut dropped = 0usize;
    for (i, ts) in timestamps.iter().enumerate() {
        let open = quote
            .open
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .map(round_price);
        let high = quote
            .high
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .map(round_price);
        let low = quote
            .low
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .map(round_price);
        let close = quote
            .close
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .map(round_price);
        let volume = quote
            .volume
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten());

        if let (Some(open), Some(high), Some(low), Some(close), Some(volume)) =
            (open, high, low, close, volume)
            && let Some(dt) = chrono::DateTime::from_timestamp(*ts, 0)
        {
            out.push(Ohlc {
                date: dt.date_naive(),
                open,
                high,
                low,
                close,
                volume,
            });
        } else {
            dropped += 1;
        }
    }

    if dropped > 0 && verbose {
        eprintln!(
            "warning: dropped {dropped} OHLC row(s) from Yahoo response due to missing fields"
        );
    }

    Ok(out)
}

fn parse_fundamentals(
    symbol: &str,
    quote_summary: &QuoteSummaryResponse,
) -> Result<Fundamentals, IdxError> {
    if let Some(err) = quote_summary.quote_summary.error.as_ref() {
        return Err(map_yahoo_error(symbol, "quoteSummary", err));
    }

    let result = quote_summary
        .quote_summary
        .result
        .as_ref()
        .and_then(|results| results.first())
        .ok_or(IdxError::ProviderUnavailable)?;

    Ok(Fundamentals {
        trailing_pe: result
            .default_key_statistics
            .get_f64("trailingPE")
            .or_else(|| result.financial_data.get_f64("trailingPE")),
        forward_pe: result
            .default_key_statistics
            .get_f64("forwardPE")
            .or_else(|| result.financial_data.get_f64("forwardPE")),
        price_to_book: result
            .default_key_statistics
            .get_f64("priceToBook")
            .or_else(|| result.financial_data.get_f64("priceToBook")),
        return_on_equity: result.financial_data.get_f64("returnOnEquity"),
        profit_margins: result.financial_data.get_f64("profitMargins"),
        return_on_assets: result.financial_data.get_f64("returnOnAssets"),
        revenue_growth: result.financial_data.get_f64("revenueGrowth"),
        earnings_growth: result
            .default_key_statistics
            .get_f64("earningsGrowth")
            .or_else(|| result.financial_data.get_f64("earningsGrowth")),
        debt_to_equity: result.financial_data.get_f64("debtToEquity"),
        current_ratio: result.financial_data.get_f64("currentRatio"),
        enterprise_value: result
            .default_key_statistics
            .get_i64("enterpriseValue")
            .or_else(|| result.financial_data.get_i64("enterpriseValue")),
        ebitda: result
            .financial_data
            .get_i64("ebitda")
            .or_else(|| result.default_key_statistics.get_i64("ebitda")),
        market_cap: result
            .financial_data
            .get_u64("marketCap")
            .or_else(|| result.default_key_statistics.get_u64("marketCap")),
    })
}

#[derive(Debug, Deserialize)]
struct ChartResponse {
    chart: ChartRoot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteSummaryResponse {
    quote_summary: QuoteSummaryRoot,
}

#[derive(Debug, Deserialize)]
struct QuoteSummaryRoot {
    result: Option<Vec<QuoteSummaryResult>>,
    error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteSummaryResult {
    #[serde(default)]
    default_key_statistics: QuoteSummarySection,
    #[serde(default)]
    financial_data: QuoteSummarySection,
}

#[derive(Debug, Deserialize)]
struct ChartRoot {
    result: Option<Vec<ChartResult>>,
    error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartError {
    code: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct ChartResult {
    meta: Option<ChartMeta>,
    timestamp: Option<Vec<i64>>,
    indicators: Option<Indicators>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ChartMeta {
    symbol: Option<String>,
    regular_market_price: Option<f64>,
    previous_close: Option<f64>,
    chart_previous_close: Option<f64>,
    regular_market_volume: Option<u64>,
    regular_market_day_high: Option<f64>,
    regular_market_day_low: Option<f64>,
    market_cap: Option<u64>,
    fifty_two_week_high: Option<f64>,
    fifty_two_week_low: Option<f64>,
    #[serde(rename = "averageDailyVolume3Month")]
    average_daily_volume_3month: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Indicators {
    quote: Option<Vec<IndicatorQuote>>,
}

#[derive(Debug, Deserialize)]
struct IndicatorQuote {
    open: Option<Vec<Option<f64>>>,
    high: Option<Vec<Option<f64>>>,
    low: Option<Vec<Option<f64>>>,
    close: Option<Vec<Option<f64>>>,
    volume: Option<Vec<Option<u64>>>,
}

type QuoteSummarySection = HashMap<String, QuoteSummaryValue>;

trait QuoteSummarySectionExt {
    fn get_f64(&self, key: &str) -> Option<f64>;
    fn get_i64(&self, key: &str) -> Option<i64>;
    fn get_u64(&self, key: &str) -> Option<u64>;
}

impl QuoteSummarySectionExt for QuoteSummarySection {
    fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(QuoteSummaryValue::as_f64)
    }

    fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(QuoteSummaryValue::as_i64)
    }

    fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(QuoteSummaryValue::as_u64)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuoteSummaryValue {
    Wrapped { raw: Option<YahooNumber> },
    Direct(YahooNumber),
    // Catch-all for empty objects {}, null, strings, booleans — return None for all numeric extractions
    Unknown(serde_json::Value),
}

impl QuoteSummaryValue {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Wrapped { raw } => raw.as_ref().map(YahooNumber::as_f64),
            Self::Direct(value) => Some(value.as_f64()),
            Self::Unknown(_) => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Wrapped { raw } => raw.as_ref().and_then(YahooNumber::as_i64),
            Self::Direct(value) => value.as_i64(),
            Self::Unknown(_) => None,
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Wrapped { raw } => raw.as_ref().and_then(YahooNumber::as_u64),
            Self::Direct(value) => value.as_u64(),
            Self::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum YahooNumber {
    I64(i64),
    U64(u64),
    F64(f64),
}

impl YahooNumber {
    fn as_f64(&self) -> f64 {
        match self {
            Self::I64(value) => *value as f64,
            Self::U64(value) => *value as f64,
            Self::F64(value) => *value,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            Self::U64(value) => i64::try_from(*value).ok(),
            Self::F64(value) => Some(value.round() as i64),
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            Self::I64(value) => u64::try_from(*value).ok(),
            Self::U64(value) => Some(*value),
            Self::F64(value) if value.is_sign_negative() => None,
            Self::F64(value) => Some(value.round() as u64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChartResponse, YahooProvider, parse_fundamentals_from_str, parse_history_from_str,
        parse_history_with_verbose, parse_quote, parse_quote_from_str,
    };

    const SAMPLE: &str = r#"{
      "chart": {
        "result": [{
          "meta": {
            "symbol": "BBCA.JK",
            "regularMarketPrice": 9875.0,
            "previousClose": 9758.0,
            "regularMarketVolume": 12300000,
            "marketCap": 1215200000000000,
            "fiftyTwoWeekHigh": 10250.0,
            "fiftyTwoWeekLow": 7800.0,
            "averageDailyVolume3Month": 10000000
          },
          "timestamp": [1709251200,1709337600],
          "indicators": {"quote":[{
            "open":[9800.0,9850.0],
            "high":[9900.0,9900.0],
            "low":[9750.0,9800.0],
            "close":[9875.0,9880.0],
            "volume":[12300000,11000000]
          }]}
        }]
      }
    }"#;

    #[test]
    fn parses_quote_and_history() {
        let chart: ChartResponse = serde_json::from_str(SAMPLE).expect("valid chart fixture");
        let quote = parse_quote("BBCA.JK", &chart).expect("quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");
        assert_eq!(quote.price, 9875);
        let history = parse_history_with_verbose(&chart, false).expect("history parsed");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].close, 9875);
    }

    #[test]
    fn parses_realistic_fixture_json() {
        let quote_raw =
            std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json").expect("fixture exists");
        let history_raw =
            std::fs::read_to_string("tests/fixtures/chart_bbca_3mo.json").expect("fixture exists");
        let fundamentals_raw = std::fs::read_to_string("tests/fixtures/quotesummary_bbca.json")
            .expect("fixture exists");

        let quote = parse_quote_from_str("BBCA.JK", &quote_raw).expect("fixture quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");
        assert_eq!(quote.market_cap, Some(1_215_200_000_000_000));
        assert_eq!(quote.avg_volume, Some(10_000_000));

        let history = parse_history_from_str(&history_raw).expect("fixture history parsed");
        assert!(!history.is_empty());

        let fundamentals = parse_fundamentals_from_str("BBCA.JK", &fundamentals_raw)
            .expect("fixture fundamentals parsed");
        assert_eq!(fundamentals.trailing_pe, Some(25.4));
        assert_eq!(fundamentals.forward_pe, Some(23.1));
        assert_eq!(fundamentals.price_to_book, Some(4.6));
        assert_eq!(fundamentals.earnings_growth, Some(0.121));
        assert_eq!(fundamentals.enterprise_value, Some(1_245_000_000_000_000));
        assert_eq!(fundamentals.ebitda, Some(58_500_000_000_000));
        assert_eq!(fundamentals.market_cap, Some(1_215_200_000_000_000));
    }

    #[test]
    fn maps_not_found_chart_error_to_symbol_not_found() {
        let raw = r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found"}}}"#;
        let err = parse_quote_from_str("INVALID.JK", raw).expect_err("expected symbol error");
        assert!(matches!(err, crate::error::IdxError::SymbolNotFound(_)));
    }

    #[test]
    fn parses_crumb_body_trimmed() {
        let crumb = YahooProvider::parse_crumb_body("  abc123xyz\n").expect("crumb should parse");
        assert_eq!(crumb, "abc123xyz");

        let empty = YahooProvider::parse_crumb_body(" \n").expect_err("empty crumb must fail");
        assert!(matches!(empty, crate::error::IdxError::Http(_)));

        let html = YahooProvider::parse_crumb_body("<html>blocked</html>")
            .expect_err("html crumb must fail");
        assert!(matches!(html, crate::error::IdxError::Http(_)));

        let rate_limited = YahooProvider::parse_crumb_body("Too Many Requests")
            .expect_err("rate limited crumb must fail");
        assert!(matches!(rate_limited, crate::error::IdxError::Http(_)));
    }
}
