use super::map::{parse_fundamentals, parse_history, parse_quote};
use super::raw_types::{KeyRatios, MsnQuote, RawChartResponse};
use crate::api::types::{Fundamentals, Ohlc, Quote};
use crate::error::IdxError;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let quotes: Vec<MsnQuote> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_quote(symbol, &quotes)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_fundamentals_from_str(
    raw: &str,
    quote_raw: Option<&str>,
) -> Result<Fundamentals, IdxError> {
    let ratios: Vec<KeyRatios> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    let quote = quote_raw
        .map(serde_json::from_str::<Vec<MsnQuote>>)
        .transpose()
        .map_err(|e| IdxError::ParseError(e.to_string()))?
        .and_then(|quotes| quotes.into_iter().next());
    parse_fundamentals(&ratios, quote.as_ref())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_history_from_str(symbol: &str, raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    let charts: Vec<RawChartResponse> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history(symbol, &charts)
}

#[cfg_attr(not(test), allow(dead_code))]
#[allow(dead_code)]
#[cfg(test)]
mod tests {
    use super::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

    fn minimal_quote_raw() -> &'static str {
        r#"[{"symbol":"BBCA","marketCap":1215200000000000}]"#
    }

    #[test]
    fn parses_quote_fixture_json() {
        let raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .expect("quote fixture exists");
        let quote = parse_quote_from_str("BBCA.JK", &raw).expect("quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");
        assert_eq!(quote.price, 9875);
        assert_eq!(quote.change, 117);
        assert_eq!(quote.volume, 12_300_000);
        assert_eq!(quote.market_cap, Some(1_215_200_000_000_000));
        assert_eq!(quote.avg_volume, Some(10_000_000));
    }

    #[test]
    fn parses_fundamentals_fixture_json() {
        let raw = std::fs::read_to_string("tests/fixtures/msn_keyratios_bbca.json")
            .expect("fundamentals fixture exists");
        let quote_raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .expect("quote fixture exists");
        let fundamentals =
            parse_fundamentals_from_str(&raw, Some(&quote_raw)).expect("fundamentals parsed");
        assert_eq!(fundamentals.trailing_pe, Some(25.4));
        assert_eq!(fundamentals.price_to_book, Some(4.6));
        assert_eq!(fundamentals.return_on_equity, Some(0.198));
        assert_eq!(fundamentals.revenue_growth, Some(0.081));
        assert_eq!(fundamentals.earnings_growth, Some(0.121));
        assert_eq!(fundamentals.market_cap, Some(1_215_200_000_000_000));
    }

    #[test]
    fn parses_history_fixture_json() {
        let raw = std::fs::read_to_string("tests/fixtures/msn_chart_bbca_3m.json")
            .expect("chart fixture exists");
        let history = parse_history_from_str("BBCA.JK", &raw).expect("chart fixture parsed");

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].date.to_string(), "2026-01-13");
        assert_eq!(history[0].close, 8000);
    }

    #[test]
    fn parses_fundamentals_with_infinity_string_as_missing_data() {
        let raw = r#"[
            {
                "companyMetrics": [
                    {
                        "year": "2025",
                        "fiscalPeriodType": "TTM",
                        "priceToEarningsRatio": "Infinity",
                        "priceToBookRatio": 4.6,
                        "roe": 19.8,
                        "profitMargin": 44.2,
                        "debtToEquityRatio": 0.75,
                        "currentRatio": 1.4
                    }
                ]
            }
        ]"#;

        let fundamentals = parse_fundamentals_from_str(raw, Some(minimal_quote_raw()))
            .expect("fundamentals parsed");

        assert_eq!(fundamentals.trailing_pe, None);
        assert_eq!(fundamentals.price_to_book, Some(4.6));
        assert_eq!(fundamentals.return_on_equity, Some(0.198));
    }

    #[test]
    fn parses_fundamentals_with_negative_infinity_string_as_missing_data() {
        let raw = r#"[
            {
                "companyMetrics": [
                    {
                        "year": "2025",
                        "fiscalPeriodType": "TTM",
                        "priceToEarningsRatio": 12.5,
                        "debtToEquityRatio": "-Infinity",
                        "roe": 19.8,
                        "profitMargin": 44.2,
                        "currentRatio": 1.4
                    }
                ]
            }
        ]"#;

        let fundamentals = parse_fundamentals_from_str(raw, Some(minimal_quote_raw()))
            .expect("fundamentals parsed");

        assert_eq!(fundamentals.trailing_pe, Some(12.5));
        assert_eq!(fundamentals.debt_to_equity, None);
        assert_eq!(fundamentals.return_on_equity, Some(0.198));
    }

    #[test]
    fn parses_fundamentals_with_nan_string_as_missing_data() {
        let raw = r#"[
            {
                "companyMetrics": [
                    {
                        "year": "2025",
                        "fiscalPeriodType": "TTM",
                        "revenueGrowthRate": "NaN",
                        "earningsGrowthRate": 12.1,
                        "roe": 19.8,
                        "profitMargin": 44.2,
                        "debtToEquityRatio": 0.75,
                        "currentRatio": 1.4
                    }
                ]
            }
        ]"#;

        let fundamentals = parse_fundamentals_from_str(raw, Some(minimal_quote_raw()))
            .expect("fundamentals parsed");

        assert_eq!(fundamentals.revenue_growth, None);
        assert_eq!(fundamentals.earnings_growth, Some(0.121));
        assert_eq!(fundamentals.return_on_equity, Some(0.198));
    }
}
