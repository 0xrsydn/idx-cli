use std::time::Duration;

use serde::Deserialize;

use crate::api::MarketDataProvider;
use crate::api::types::{Interval, Ohlc, Period, Quote};
use crate::error::IdxError;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
// query2 works without crumb/cookie auth from datacenter IPs; query1 requires consent cookies
const BASE_URL: &str = "https://query2.finance.yahoo.com";

pub struct YahooProvider {
    agent: ureq::Agent,
    verbose: bool,
}

impl YahooProvider {
    pub fn new(verbose: bool) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .build()
            .into();

        Self { agent, verbose }
    }

    fn chart_url(symbol: &str, period: &Period, interval: &Interval) -> String {
        format!(
            "{BASE_URL}/v8/finance/chart/{symbol}?range={}&interval={}",
            period.as_str(),
            interval.as_str()
        )
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
                        return Err(map_chart_error(symbol, err));
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
}

fn jitter() -> Duration {
    Duration::from_millis(fastrand::u64(0..100))
}

fn round_price(value: f64) -> i64 {
    value.round() as i64
}

// verbose behavior is configured on YahooProvider and threaded into history parsing.

fn map_chart_error(symbol: &str, err: &ChartError) -> IdxError {
    if err.code.eq_ignore_ascii_case("Not Found") {
        return IdxError::SymbolNotFound(symbol.to_string());
    }
    IdxError::Http(format!(
        "yahoo chart error {}: {}",
        err.code, err.description
    ))
}

impl MarketDataProvider for YahooProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let chart = self.fetch_chart(symbol, &Period::OneDay, &Interval::Day)?;
        parse_quote(symbol, &chart)
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
        return Err(map_chart_error(symbol, err));
    }
    parse_quote(symbol, &chart)
}

fn parse_quote(symbol: &str, chart: &ChartResponse) -> Result<Quote, IdxError> {
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_chart_error(symbol, err));
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

fn parse_history_with_verbose(chart: &ChartResponse, verbose: bool) -> Result<Vec<Ohlc>, IdxError> {
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_chart_error("unknown", err));
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

#[derive(Debug, Deserialize)]
struct ChartResponse {
    chart: ChartRoot,
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

#[cfg(test)]
mod tests {
    use super::{
        ChartResponse, parse_history_from_str, parse_history_with_verbose, parse_quote,
        parse_quote_from_str,
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

        let quote = parse_quote_from_str("BBCA.JK", &quote_raw).expect("fixture quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");
        assert_eq!(quote.market_cap, Some(1_215_200_000_000_000));
        assert_eq!(quote.avg_volume, Some(10_000_000));

        let history = parse_history_from_str(&history_raw).expect("fixture history parsed");
        assert!(!history.is_empty());
    }

    #[test]
    fn maps_not_found_chart_error_to_symbol_not_found() {
        let raw = r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found"}}}"#;
        let err = parse_quote_from_str("INVALID.JK", raw).expect_err("expected symbol error");
        assert!(matches!(err, crate::error::IdxError::SymbolNotFound(_)));
    }
}
