use chrono::NaiveDate;

use super::map::{parse_fundamentals, parse_history, parse_history_with_drop_count, parse_quote};
use super::raw_types::{KeyRatios, MsnChart, MsnQuote};
use crate::api::types::{Fundamentals, Ohlc, Period, Quote};
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
pub(crate) fn parse_history_from_str(period: &Period, raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    let charts: Vec<MsnChart> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history(period, &charts)
}

#[allow(dead_code)] // retained for verbose history path, wired once MSN charts are exposed
pub(super) fn parse_history_with_verbose(
    period: &Period,
    charts: &[MsnChart],
    verbose: bool,
) -> Result<Vec<Ohlc>, IdxError> {
    let (history, dropped) = parse_history_with_drop_count(period, charts)?;
    if dropped > 0 && verbose {
        eprintln!("warning: dropped {dropped} OHLC row(s) from MSN response due to missing fields");
    }
    Ok(history)
}

#[allow(dead_code)]
fn parse_close_only_history_from_str(
    period: &Period,
    raw: &str,
) -> Result<Vec<ClosePoint>, IdxError> {
    let charts: Vec<MsnChart> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_close_only_history(period, &charts)
}

fn parse_close_only_history(
    period: &Period,
    charts: &[MsnChart],
) -> Result<Vec<ClosePoint>, IdxError> {
    let chart = charts.first().ok_or(IdxError::ProviderUnavailable)?;
    let timestamps = &chart.series.time_stamps;
    let mut grouped: std::collections::BTreeMap<NaiveDate, ClosePoint> =
        std::collections::BTreeMap::new();

    for (idx, raw_ts) in timestamps.iter().enumerate() {
        let Some(date) = chrono::DateTime::parse_from_rfc3339(raw_ts)
            .map(|d| d.date_naive())
            .ok()
            .or_else(|| {
                raw_ts
                    .parse::<i64>()
                    .ok()
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|d| d.date_naive()))
            })
            .or_else(|| NaiveDate::parse_from_str(raw_ts, "%Y-%m-%d").ok())
        else {
            continue;
        };
        let Some(close) = chart.series.prices.get(idx).copied() else {
            continue;
        };

        grouped.insert(
            date,
            ClosePoint {
                date,
                close: close.round() as i64,
            },
        );
    }

    let mut out: Vec<ClosePoint> = grouped.into_values().collect();
    trim_close_history_to_period(period, &mut out);

    if out.is_empty() {
        return Err(IdxError::ProviderUnavailable);
    }

    Ok(out)
}

fn trim_close_history_to_period(period: &Period, rows: &mut Vec<ClosePoint>) {
    let days: i64 = match period {
        Period::OneDay => return,
        Period::FiveDays => 5,
        Period::OneMonth => 31,
        Period::ThreeMonths => 92,
        Period::SixMonths => 183,
        Period::OneYear => 366,
        Period::TwoYears => 731,
        Period::FiveYears => 1826,
    };

    let Some(last_date) = rows.last().map(|item| item.date) else {
        return;
    };
    let cutoff = last_date - chrono::Duration::days(days.saturating_sub(1));
    rows.retain(|item| item.date >= cutoff);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClosePoint {
    date: NaiveDate,
    close: i64,
}

#[cfg(test)]
mod tests {
    use super::{
        parse_close_only_history_from_str, parse_fundamentals_from_str, parse_history_from_str,
        parse_quote_from_str,
    };
    use crate::api::types::{Ohlc, Period};

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
        let raw = std::fs::read_to_string("tests/fixtures/msn_chart_bbca_3mo.json")
            .expect("history fixture exists");
        let history = parse_history_from_str(&Period::ThreeMonths, &raw).expect("history parsed");
        assert_eq!(history.len(), 6);
        assert_eq!(history[0].date.to_string(), "2025-01-06");
        assert_eq!(history[0].open, 9800);
        assert_eq!(history[0].close, 9875);
        assert_eq!(history[5].close, 9940);
    }

    #[test]
    fn rejects_close_only_chart_series_for_public_history() {
        let raw = r#"[
          {
            "series": {
              "prices": [7100.0, 7200.0],
              "timeStamps": ["2026-03-03T17:00:00Z", "2026-03-04T17:00:00Z"]
            }
          }
        ]"#;
        let err =
            parse_history_from_str(&Period::ThreeMonths, raw).expect_err("history should fail");
        assert_eq!(
            err.to_string(),
            "parse error: msn does not expose real OHLC/volume for this history range"
        );
    }

    #[test]
    fn parses_close_only_series_for_internal_use() {
        let raw = r#"[
          {
            "series": {
              "prices": [7100.0, 7200.0],
              "timeStamps": ["2026-03-03T17:00:00Z", "2026-03-04T17:00:00Z"]
            }
          }
        ]"#;
        let history =
            parse_close_only_history_from_str(&Period::ThreeMonths, raw).expect("history parsed");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].close, 7100);
        assert_eq!(history[1].close, 7200);
    }

    #[test]
    fn resamples_history_to_weekly_bars() {
        let rows = vec![
            Ohlc {
                date: chrono::NaiveDate::from_ymd_opt(2025, 1, 6).expect("date"),
                open: 100,
                high: 110,
                low: 90,
                close: 105,
                volume: 10,
            },
            Ohlc {
                date: chrono::NaiveDate::from_ymd_opt(2025, 1, 7).expect("date"),
                open: 106,
                high: 111,
                low: 101,
                close: 109,
                volume: 11,
            },
            Ohlc {
                date: chrono::NaiveDate::from_ymd_opt(2025, 1, 13).expect("date"),
                open: 110,
                high: 115,
                low: 108,
                close: 114,
                volume: 12,
            },
        ];

        let weekly =
            super::super::map::resample_history(&rows, super::super::map::ResampleInterval::Week);
        assert_eq!(weekly.len(), 2);
        assert_eq!(weekly[0].open, 100);
        assert_eq!(weekly[0].close, 109);
        assert_eq!(weekly[0].volume, 21);
        assert_eq!(weekly[1].close, 114);
    }
}
