use std::collections::HashMap;

use serde::Deserialize;

use crate::api::types::{Fundamentals, Ohlc, Quote};
use crate::error::IdxError;

pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_yahoo_error(symbol, "chart", err));
    }
    parse_quote(symbol, &chart)
}

pub(super) fn parse_quote(symbol: &str, chart: &ChartResponse) -> Result<Quote, IdxError> {
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
        .ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
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

pub(super) fn parse_history_with_verbose(
    chart: &ChartResponse,
    verbose: bool,
) -> Result<Vec<Ohlc>, IdxError> {
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

pub(super) fn parse_fundamentals(
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

fn round_price(value: f64) -> i64 {
    value.round() as i64
}

pub(super) fn map_yahoo_error(symbol: &str, endpoint: &str, err: &ChartError) -> IdxError {
    if err.code.eq_ignore_ascii_case("Not Found") {
        return IdxError::SymbolNotFound(symbol.to_string());
    }
    IdxError::Http(format!(
        "yahoo {endpoint} error {}: {}",
        err.code, err.description
    ))
}

#[derive(Debug, Deserialize)]
pub(super) struct ChartResponse {
    chart: ChartRoot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuoteSummaryResponse {
    quote_summary: QuoteSummaryRoot,
}

#[derive(Debug, Deserialize)]
pub(super) struct QuoteSummaryRoot {
    result: Option<Vec<QuoteSummaryResult>>,
    error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuoteSummaryResult {
    #[serde(default)]
    default_key_statistics: QuoteSummarySection,
    #[serde(default)]
    financial_data: QuoteSummarySection,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChartRoot {
    result: Option<Vec<ChartResult>>,
    error: Option<ChartError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChartError {
    code: String,
    description: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChartResult {
    meta: Option<ChartMeta>,
    timestamp: Option<Vec<i64>>,
    indicators: Option<Indicators>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(super) struct ChartMeta {
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
pub(super) struct Indicators {
    quote: Option<Vec<IndicatorQuote>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct IndicatorQuote {
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
#[allow(dead_code)]
enum QuoteSummaryValue {
    Wrapped { raw: Option<YahooNumber> },
    Direct(YahooNumber),
    // Catch-all for empty objects {}, null, strings, booleans; return None for numeric extractions.
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
        ChartResponse, parse_fundamentals_from_str, parse_history_from_str,
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
}
