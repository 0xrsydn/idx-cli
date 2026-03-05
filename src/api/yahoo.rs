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
}

impl YahooProvider {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_defaults(),
        }
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
                    return ok
                        .into_body()
                        .read_json::<ChartResponse>()
                        .map_err(|e| IdxError::ParseError(e.to_string()));
                }
                Err(ureq::Error::StatusCode(429)) => {
                    if attempt < 2 {
                        std::thread::sleep(wait + jitter());
                        wait *= 2;
                    }
                }
                Err(e) => return Err(IdxError::Http(e.to_string())),
            }
        }
        Err(IdxError::RateLimited)
    }
}

fn jitter() -> Duration {
    let millis = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() % 100)
        .unwrap_or(42)) as u64;
    Duration::from_millis(millis)
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
        parse_history(&chart)
    }
}

pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_quote(symbol, &chart)
}

fn parse_quote(symbol: &str, chart: &ChartResponse) -> Result<Quote, IdxError> {
    let result = chart
        .chart
        .result
        .as_ref()
        .and_then(|r| r.first())
        .ok_or(IdxError::ProviderUnavailable)?;
    let meta = result.meta.as_ref().ok_or(IdxError::ProviderUnavailable)?;
    let price = meta
        .regular_market_price
        .ok_or(IdxError::SymbolNotFound(symbol.to_string()))?;
    let prev_close = meta.previous_close.or(meta.chart_previous_close);
    let change = prev_close.map_or(0.0, |p| price - p);
    let change_pct = prev_close.map_or(0.0, |p| if p != 0.0 { (change / p) * 100.0 } else { 0.0 });

    let (week52_position, range_signal) = match (meta.fifty_two_week_low, meta.fifty_two_week_high)
    {
        (Some(low), Some(high)) if high > low => {
            let pos = (price - low) / (high - low);
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
        week52_high: meta.fifty_two_week_high,
        week52_low: meta.fifty_two_week_low,
        week52_position,
        range_signal,
        prev_close,
        avg_volume: meta.average_daily_volume_3month,
    })
}

pub(crate) fn parse_history_from_str(raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history(&chart)
}

fn parse_history(chart: &ChartResponse) -> Result<Vec<Ohlc>, IdxError> {
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
    for (i, ts) in timestamps.iter().enumerate() {
        let open = quote
            .open
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten());
        let high = quote
            .high
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten());
        let low = quote.low.as_ref().and_then(|v| v.get(i).copied().flatten());
        let close = quote
            .close
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten());
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
        }
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
    market_cap: Option<f64>,
    fifty_two_week_high: Option<f64>,
    fifty_two_week_low: Option<f64>,
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
        ChartResponse, parse_history, parse_history_from_str, parse_quote, parse_quote_from_str,
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
        assert_eq!(quote.price, 9875.0);
        let history = parse_history(&chart).expect("history parsed");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].close, 9875.0);
    }

    #[test]
    fn parses_realistic_fixture_json() {
        let quote_raw =
            std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json").expect("fixture exists");
        let history_raw =
            std::fs::read_to_string("tests/fixtures/chart_bbca_3mo.json").expect("fixture exists");

        let quote = parse_quote_from_str("BBCA.JK", &quote_raw).expect("fixture quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");

        let history = parse_history_from_str(&history_raw).expect("fixture history parsed");
        assert!(!history.is_empty());
    }
}
