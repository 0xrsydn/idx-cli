use crate::api::types::{Fundamentals, Ohlc, Quote};
use crate::error::IdxError;

use super::map::{parse_fundamentals, parse_history, parse_quote};
use super::raw_types::{ChartResponse, QuoteSummaryResponse};

pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_quote(symbol, &chart)
}

#[cfg(test)]
pub(crate) fn parse_history_from_str(symbol: &str, raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    parse_history_from_str_with_verbose(symbol, raw, false)
}

pub(crate) fn parse_history_from_str_with_verbose(
    symbol: &str,
    raw: &str,
    verbose: bool,
) -> Result<Vec<Ohlc>, IdxError> {
    let chart: ChartResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history_with_verbose(symbol, &chart, verbose)
}

pub(crate) fn parse_fundamentals_from_str(
    symbol: &str,
    raw: &str,
) -> Result<Fundamentals, IdxError> {
    let quote_summary: QuoteSummaryResponse =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_fundamentals(symbol, &quote_summary)
}

pub(super) fn parse_history_with_verbose(
    symbol: &str,
    chart: &ChartResponse,
    verbose: bool,
) -> Result<Vec<Ohlc>, IdxError> {
    let (history, dropped) = parse_history(symbol, chart)?;
    if dropped > 0 && verbose {
        crate::runtime::warn(format!(
            "dropped {dropped} OHLC row(s) from Yahoo response due to missing fields"
        ));
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::{
        ChartResponse, parse_fundamentals_from_str, parse_history_from_str,
        parse_history_from_str_with_verbose, parse_history_with_verbose, parse_quote,
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
        let history = parse_history_with_verbose("BBCA.JK", &chart, false).expect("history parsed");
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

        let history =
            parse_history_from_str("BBCA.JK", &history_raw).expect("fixture history parsed");
        assert!(!history.is_empty());

        let verbose_history = parse_history_from_str_with_verbose("BBCA.JK", &history_raw, true)
            .expect("fixture history parsed in verbose mode");
        assert_eq!(verbose_history.len(), history.len());
        assert_eq!(verbose_history[0].close, history[0].close);

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
