use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};

use super::raw_types::{IndustryMetric, KeyRatios, MsnChart, MsnQuote};
use super::symbols::{normalized_symbol, ticker_from_symbol};
use crate::api::types::{Fundamentals, Ohlc, Period, Quote};
use crate::error::IdxError;

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

pub(super) fn parse_history(period: &Period, charts: &[MsnChart]) -> Result<Vec<Ohlc>, IdxError> {
    parse_history_with_drop_count(period, charts).map(|v| v.0)
}

pub(super) fn parse_history_with_drop_count(
    period: &Period,
    charts: &[MsnChart],
) -> Result<(Vec<Ohlc>, usize), IdxError> {
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

    if out.is_empty() {
        return Err(IdxError::ProviderUnavailable);
    }

    Ok((out, dropped))
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

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(super) enum ResampleInterval {
    Week,
    Month,
}

#[allow(dead_code)]
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
