use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use super::symbols::{normalized_symbol, ticker_from_symbol};
use crate::api::types::{Fundamentals, Ohlc, Period, Quote};
use crate::error::IdxError;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let quotes: Vec<MsnQuote> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_quote(symbol, &quotes)
}

pub(super) fn parse_quote(symbol: &str, quotes: &[MsnQuote]) -> Result<Quote, IdxError> {
    let quote = quotes.first().ok_or(IdxError::ProviderUnavailable)?;
    let raw_price = quote
        .price
        .ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
    let prev_close = quote.price_previous_close.map(round_price);
    let price = round_price(raw_price);
    let change = prev_close
        .map(|previous| price - previous)
        .or_else(|| quote.price_change.map(round_price))
        .unwrap_or(0);

    let ticker = quote
        .symbol
        .as_deref()
        .and_then(ticker_from_symbol)
        .unwrap_or_else(|| ticker_from_symbol(symbol).unwrap_or_default());

    let (week52_position, range_signal) = match (quote.price_52w_low, quote.price_52w_high) {
        (Some(low), Some(high)) if high > low => {
            let position = (raw_price - low) / (high - low);
            let signal = if position > 0.66 {
                Some("upper".to_string())
            } else if position < 0.33 {
                Some("lower".to_string())
            } else {
                Some("middle".to_string())
            };
            (Some(position), signal)
        }
        _ => (None, None),
    };

    Ok(Quote {
        symbol: normalized_symbol(symbol, &ticker),
        price,
        change,
        change_pct: quote.price_change_percent.unwrap_or(0.0),
        volume: round_u64(quote.accumulated_volume).unwrap_or(0),
        market_cap: round_u64(quote.market_cap),
        week52_high: quote.price_52w_high.map(round_price),
        week52_low: quote.price_52w_low.map(round_price),
        week52_position,
        range_signal,
        prev_close,
        avg_volume: round_u64(quote.average_volume),
    })
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

pub(super) fn parse_fundamentals(
    ratios: &[KeyRatios],
    quote: Option<&MsnQuote>,
) -> Result<Fundamentals, IdxError> {
    let ratios = ratios.first().ok_or(IdxError::ProviderUnavailable)?;
    let metrics = if ratios.company_metrics.is_empty() {
        &ratios.industry_metrics
    } else {
        &ratios.company_metrics
    };
    if preferred_metric(metrics).is_none() {
        return Err(IdxError::ProviderUnavailable);
    }

    Ok(Fundamentals {
        trailing_pe: best_metric_value(metrics, |metric| metric.price_to_earnings_ratio),
        forward_pe: best_metric_value(metrics, |metric| metric.forward_price_to_eps),
        price_to_book: best_metric_value(metrics, |metric| metric.price_to_book_ratio),
        return_on_equity: best_metric_value(metrics, |metric| normalize_percentish(metric.roe)),
        profit_margins: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.profit_margin.or(metric.net_margin))
        }),
        return_on_assets: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.roa_ttm.or(metric.return_on_asset_current))
        }),
        revenue_growth: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.revenue_ytd_ytd.or(metric.revenue_growth_rate))
        }),
        earnings_growth: best_metric_value(metrics, |metric| {
            normalize_percentish(
                metric
                    .net_income_ytd_ytd_growth_rate
                    .or(metric.earnings_growth_rate),
            )
        }),
        debt_to_equity: best_metric_value(metrics, |metric| metric.debt_to_equity_ratio),
        current_ratio: best_metric_value(metrics, |metric| {
            sanitize_current_ratio(metric.current_ratio)
        }),
        enterprise_value: None,
        ebitda: None,
        market_cap: quote.and_then(|item| round_u64(item.market_cap)),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_history_from_str(period: &Period, raw: &str) -> Result<Vec<Ohlc>, IdxError> {
    let charts: Vec<MsnChart> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_history_with_verbose(period, &charts, false)
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

pub(super) fn parse_history_with_verbose(
    period: &Period,
    charts: &[MsnChart],
    verbose: bool,
) -> Result<Vec<Ohlc>, IdxError> {
    let chart = charts.first().ok_or(IdxError::ProviderUnavailable)?;

    if !chart.series.has_real_ohlcv() {
        return Err(IdxError::ParseError(
            "msn does not expose real OHLC/volume for this history range".to_string(),
        ));
    }

    let timestamps = &chart.series.time_stamps;

    let mut grouped: BTreeMap<NaiveDate, Ohlc> = BTreeMap::new();
    let mut dropped = 0usize;

    for (idx, raw_ts) in timestamps.iter().enumerate() {
        let Some(date) = parse_chart_date(raw_ts) else {
            dropped += 1;
            continue;
        };
        let point = (
            chart.series.open_prices.get(idx).copied(),
            chart.series.prices_high.get(idx).copied(),
            chart.series.prices_low.get(idx).copied(),
            chart.series.prices.get(idx).copied(),
            chart.series.volumes.get(idx).copied(),
        );

        let (Some(open), Some(high), Some(low), Some(close), Some(volume)) = point else {
            dropped += 1;
            continue;
        };

        let candle = Ohlc {
            date,
            open: round_price(open),
            high: round_price(high),
            low: round_price(low),
            close: round_price(close),
            volume: round_u64(Some(volume)).unwrap_or(0),
        };

        grouped
            .entry(date)
            .and_modify(|existing| {
                existing.high = existing.high.max(candle.high);
                existing.low = existing.low.min(candle.low);
                existing.close = candle.close;
                existing.volume = existing.volume.saturating_add(candle.volume);
            })
            .or_insert(candle);
    }

    let mut out: Vec<Ohlc> = grouped.into_values().collect();
    trim_history_to_period(period, &mut out);

    if dropped > 0 && verbose {
        eprintln!("warning: dropped {dropped} OHLC row(s) from MSN response due to missing fields");
    }

    if out.is_empty() {
        return Err(IdxError::ProviderUnavailable);
    }

    Ok(out)
}

fn parse_close_only_history(
    period: &Period,
    charts: &[MsnChart],
) -> Result<Vec<ClosePoint>, IdxError> {
    let chart = charts.first().ok_or(IdxError::ProviderUnavailable)?;
    let timestamps = &chart.series.time_stamps;
    let mut grouped: BTreeMap<NaiveDate, ClosePoint> = BTreeMap::new();

    for (idx, raw_ts) in timestamps.iter().enumerate() {
        let Some(date) = parse_chart_date(raw_ts) else {
            continue;
        };
        let Some(close) = chart.series.prices.get(idx).copied() else {
            continue;
        };

        grouped.insert(
            date,
            ClosePoint {
                date,
                close: round_price(close),
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

fn trim_history_to_period(period: &Period, rows: &mut Vec<Ohlc>) {
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

fn preferred_metric(metrics: &[IndustryMetric]) -> Option<&IndustryMetric> {
    metrics.iter().max_by_key(|metric| metric_rank(metric))
}

fn best_metric_value<T: Copy>(
    metrics: &[IndustryMetric],
    extractor: impl Fn(&IndustryMetric) -> Option<T>,
) -> Option<T> {
    metrics
        .iter()
        .filter_map(|metric| extractor(metric).map(|value| (metric_rank(metric), value)))
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, value)| value)
}

fn metric_rank(metric: &IndustryMetric) -> (i32, i32) {
    (
        metric
            .year
            .as_deref()
            .and_then(|year| year.parse::<i32>().ok())
            .unwrap_or(i32::MIN),
        metric_period_priority(metric.fiscal_period_type.as_deref()),
    )
}

fn metric_period_priority(period: Option<&str>) -> i32 {
    match period.map(|value| value.trim()) {
        Some(value) if value.eq_ignore_ascii_case("TTM") => 7,
        Some(value)
            if value.eq_ignore_ascii_case("ANNUAL")
                || value.eq_ignore_ascii_case("FY")
                || value.eq_ignore_ascii_case("YEAR") =>
        {
            6
        }
        Some(value) if value.eq_ignore_ascii_case("Q4") => 5,
        Some(value) if value.eq_ignore_ascii_case("Q3") => 4,
        Some(value) if value.eq_ignore_ascii_case("Q2") => 3,
        Some(value) if value.eq_ignore_ascii_case("Q1") => 2,
        Some(value) if value.eq_ignore_ascii_case("NTM") => 1,
        _ => 0,
    }
}

fn normalize_percentish(value: Option<f64>) -> Option<f64> {
    value.and_then(|number| {
        if !number.is_finite() {
            None
        } else if number.abs() > 1.0 {
            Some(number / 100.0)
        } else {
            Some(number)
        }
    })
}

fn sanitize_current_ratio(value: Option<f64>) -> Option<f64> {
    value.and_then(|number| {
        if !number.is_finite() || number < 0.01 {
            None
        } else {
            Some(number)
        }
    })
}

#[derive(Clone, Copy)]
pub(super) enum ResampleInterval {
    Week,
    Month,
}

pub(super) fn resample_history(rows: &[Ohlc], interval: ResampleInterval) -> Vec<Ohlc> {
    let mut grouped: BTreeMap<(i32, u32), Ohlc> = BTreeMap::new();

    for row in rows {
        let key = match interval {
            ResampleInterval::Week => {
                let iso = row.date.iso_week();
                (iso.year(), iso.week())
            }
            ResampleInterval::Month => (row.date.year(), row.date.month()),
        };

        grouped
            .entry(key)
            .and_modify(|existing| {
                existing.high = existing.high.max(row.high);
                existing.low = existing.low.min(row.low);
                existing.close = row.close;
                existing.volume = existing.volume.saturating_add(row.volume);
                existing.date = row.date;
            })
            .or_insert_with(|| row.clone());
    }

    grouped.into_values().collect()
}

fn parse_chart_date(raw: &str) -> Option<NaiveDate> {
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(date.date_naive());
    }
    if let Ok(timestamp) = raw.parse::<i64>() {
        return chrono::DateTime::from_timestamp(timestamp, 0).map(|dt| dt.date_naive());
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn round_price(value: f64) -> i64 {
    value.round() as i64
}

fn round_u64(value: Option<f64>) -> Option<u64> {
    value.and_then(|number| {
        if !number.is_finite() || number.is_sign_negative() {
            None
        } else {
            Some(number.round() as u64)
        }
    })
}

fn de_opt_f64_lenient<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberLike {
        F64(f64),
        String(String),
    }

    let value = Option::<NumberLike>::deserialize(deserializer)?;
    match value {
        Some(NumberLike::F64(number)) if number.is_finite() => Ok(Some(number)),
        Some(NumberLike::F64(_)) => Ok(None),
        Some(NumberLike::String(raw)) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("nan") {
                Ok(None)
            } else {
                trimmed.parse::<f64>().map(Some).map_err(D::Error::custom)
            }
        }
        None => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MsnQuote {
    #[serde(default)]
    symbol: Option<String>,
    price: Option<f64>,
    #[serde(default)]
    price_change: Option<f64>,
    #[serde(default)]
    price_change_percent: Option<f64>,
    #[serde(default)]
    price_previous_close: Option<f64>,
    #[serde(default, rename = "price52wHigh")]
    price_52w_high: Option<f64>,
    #[serde(default, rename = "price52wLow")]
    price_52w_low: Option<f64>,
    #[serde(default)]
    accumulated_volume: Option<f64>,
    #[serde(default)]
    average_volume: Option<f64>,
    #[serde(default)]
    market_cap: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyRatios {
    #[serde(default)]
    industry_metrics: Vec<IndustryMetric>,
    #[serde(default)]
    company_metrics: Vec<IndustryMetric>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndustryMetric {
    year: Option<String>,
    fiscal_period_type: Option<String>,
    #[serde(default)]
    revenue_growth_rate: Option<f64>,
    #[serde(default)]
    earnings_growth_rate: Option<f64>,
    #[serde(default, rename = "netIncomeYTDYTDGrowthRate")]
    net_income_ytd_ytd_growth_rate: Option<f64>,
    #[serde(default, rename = "revenueYTDYTD")]
    revenue_ytd_ytd: Option<f64>,
    #[serde(default)]
    net_margin: Option<f64>,
    #[serde(default)]
    profit_margin: Option<f64>,
    #[serde(default)]
    roe: Option<f64>,
    #[serde(default, rename = "roaTTM")]
    roa_ttm: Option<f64>,
    #[serde(default)]
    return_on_asset_current: Option<f64>,
    #[serde(default)]
    debt_to_equity_ratio: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_f64_lenient")]
    current_ratio: Option<f64>,
    #[serde(default)]
    price_to_earnings_ratio: Option<f64>,
    #[serde(default, rename = "forwardPriceToEPS")]
    forward_price_to_eps: Option<f64>,
    #[serde(default)]
    price_to_book_ratio: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MsnChart {
    series: ChartSeries,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartSeries {
    #[serde(default)]
    time_stamps: Vec<String>,
    #[serde(default)]
    prices: Vec<f64>,
    #[serde(default)]
    open_prices: Vec<f64>,
    #[serde(default)]
    prices_high: Vec<f64>,
    #[serde(default)]
    prices_low: Vec<f64>,
    #[serde(default)]
    volumes: Vec<f64>,
}

impl ChartSeries {
    fn has_real_ohlcv(&self) -> bool {
        !self.time_stamps.is_empty()
            && self.open_prices.len() == self.time_stamps.len()
            && self.prices_high.len() == self.time_stamps.len()
            && self.prices_low.len() == self.time_stamps.len()
            && self.prices.len() == self.time_stamps.len()
            && self.volumes.len() == self.time_stamps.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClosePoint {
    date: NaiveDate,
    close: i64,
}

#[cfg(test)]
mod tests {
    use super::{
        ResampleInterval, parse_close_only_history_from_str, parse_fundamentals_from_str,
        parse_history_from_str, parse_quote_from_str, resample_history,
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

        let weekly = resample_history(&rows, ResampleInterval::Week);
        assert_eq!(weekly.len(), 2);
        assert_eq!(weekly[0].open, 100);
        assert_eq!(weekly[0].close, 109);
        assert_eq!(weekly[0].volume, 21);
        assert_eq!(weekly[1].close, 114);
    }

    #[test]
    fn normalizes_live_style_percent_metrics() {
        let raw = r#"[
          {
            "industryMetrics": [
              {
                "year": "2025",
                "fiscalPeriodType": "Q1",
                "revenueGrowthRate": 9.584679119559473,
                "earningsGrowthRate": 28.793562408178182,
                "netMargin": 35.05868669243578,
                "roe": 16.27117054525313,
                "returnOnAssetCurrent": 2.5707368150889867,
                "debtToEquityRatio": 32.80253090283387,
                "currentRatio": 9.38775908812586E-06,
                "priceToEarningsRatio": 21.331183408517173,
                "priceToBookRatio": 3.0625539678152234
              },
              {
                "year": "2025",
                "fiscalPeriodType": "TTM",
                "revenueYTDYTD": 0.0481563350951302,
                "netIncomeYTDYTDGrowthRate": 0.0492553610240516,
                "profitMargin": 0.504190105842766,
                "roe": 0.211493,
                "roaTTM": 3.7919,
                "priceToEarningsRatio": 17.296683642049683,
                "priceToSalesRatio": 7.6652108104296985,
                "priceToBookRatio": 3.107795874896335
              },
              {
                "year": "2025",
                "fiscalPeriodType": "NTM",
                "forwardPriceToEPS": 14.723
              }
            ],
            "companyMetrics": [
              {
                "year": "2025",
                "fiscalPeriodType": "TTM",
                "revenueYTDYTD": 0.0481563350951302,
                "netIncomeYTDYTDGrowthRate": 0.0492553610240516,
                "profitMargin": 0.504190105842766,
                "roe": 0.211493,
                "roaTTM": 3.7919,
                "priceToEarningsRatio": 17.296683642049683,
                "priceToBookRatio": 3.107795874896335
              },
              {
                "year": "2025",
                "fiscalPeriodType": "NTM",
                "forwardPriceToEPS": 14.723
              }
            ]
          }
        ]"#;
        let quote_raw = r#"[{"symbol":"BBCA","marketCap":866500400000000.0}]"#;

        let fundamentals =
            parse_fundamentals_from_str(raw, Some(quote_raw)).expect("fundamentals parsed");
        assert_eq!(fundamentals.trailing_pe, Some(17.296683642049683));
        assert_eq!(fundamentals.forward_pe, Some(14.723));
        assert_eq!(fundamentals.price_to_book, Some(3.107795874896335));
        assert_eq!(fundamentals.return_on_equity, Some(0.211493));
        assert_eq!(fundamentals.profit_margins, Some(0.504190105842766));
        assert_eq!(fundamentals.return_on_assets, Some(0.037919));
        assert_eq!(fundamentals.revenue_growth, Some(0.0481563350951302));
        assert_eq!(fundamentals.earnings_growth, Some(0.0492553610240516));
        assert_eq!(fundamentals.debt_to_equity, None);
        assert_eq!(fundamentals.current_ratio, None);
    }
}
