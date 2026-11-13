use crate::api::types::{Fundamentals, Ohlc, Quote};
use crate::error::IdxError;

use super::raw_types::{ChartError, ChartResponse, QuoteSummaryResponse};

/// The close the day's change is measured from. Yahoo's
/// `chartPreviousClose` can be a session behind for an index (`^JKSE` on
/// 2026-11-13 carried the close of two sessions before, beside a correct
/// `regularMarketChangePercent`), so when the two disagree the close is
/// worked back from Yahoo's own percentage.
fn previous_close(price: f64, stated: Option<f64>, change_pct: Option<f64>) -> Option<f64> {
    const TOLERANCE_PCT: f64 = 0.05;
    let Some(pct) = change_pct.filter(|pct| pct.is_finite() && *pct > -100.0) else {
        return stated;
    };
    let agrees = stated
        .filter(|close| *close != 0.0)
        .is_some_and(|close| ((price - close) / close * 100.0 - pct).abs() <= TOLERANCE_PCT);
    if agrees {
        stated
    } else {
        Some(price / (1.0 + pct / 100.0))
    }
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
    let raw_prev_close = previous_close(
        raw_price,
        meta.previous_close.or(meta.chart_previous_close),
        meta.regular_market_change_percent,
    );

    let price = round_price(raw_price).ok_or_else(|| {
        IdxError::ParseError(format!(
            "Yahoo sent an implausible price for {symbol}: {raw_price}"
        ))
    })?;
    let prev_close = raw_prev_close.and_then(round_price);
    let change = prev_close.map(|p| price - p);
    let change_pct = raw_prev_close
        .filter(|p| *p != 0.0)
        .map(|p| ((raw_price - p) / p) * 100.0);

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
        volume: meta.regular_market_volume,
        market_cap: meta.market_cap,
        week52_high: meta.fifty_two_week_high.and_then(round_price),
        week52_low: meta.fifty_two_week_low.and_then(round_price),
        week52_position,
        range_signal,
        prev_close,
        avg_volume: meta.average_daily_volume_3month,
        as_of: meta
            .regular_market_time
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|t| t.with_timezone(&crate::api::idx_offset())),
        behind_session: false,
        bid: None,
        offer: None,
        market_status: None,
        session_note: None,
    })
}

pub(super) fn parse_history(
    symbol: &str,
    chart: &ChartResponse,
) -> Result<(Vec<Ohlc>, usize), IdxError> {
    if let Some(err) = chart.chart.error.as_ref() {
        return Err(map_yahoo_error(symbol, "chart", err));
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
    // Yahoo stamps weekly/monthly bars at local midnight (17:00Z the day before),
    // so take the date in the exchange's own offset, not UTC.
    let exchange_tz = result
        .meta
        .as_ref()
        .and_then(|m| m.gmtoffset)
        .and_then(chrono::FixedOffset::east_opt)
        .unwrap_or_else(crate::api::idx_offset);

    let mut out = Vec::new();
    let mut dropped = 0usize;
    for (i, ts) in timestamps.iter().enumerate() {
        let open = quote
            .open
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .and_then(round_price);
        let high = quote
            .high
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .and_then(round_price);
        let low = quote
            .low
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .and_then(round_price);
        let close = quote
            .close
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten())
            .and_then(round_price);
        let volume = quote
            .volume
            .as_ref()
            .and_then(|v| v.get(i).copied().flatten());

        if let (Some(open), Some(high), Some(low), Some(close), Some(volume)) =
            (open, high, low, close, volume)
            && let Some(dt) = chrono::DateTime::from_timestamp(*ts, 0)
        {
            out.push(Ohlc {
                date: dt.with_timezone(&exchange_tz).date_naive(),
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

    Ok((out, dropped))
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

    let stats = result.default_key_statistics.as_ref();
    let fin = result.financial_data.as_ref();
    let summary = result.summary_detail.as_ref();

    Ok(Fundamentals {
        trailing_pe: stats
            .and_then(|s| s.trailing_pe.as_ref().and_then(|v| v.raw))
            .or_else(|| fin.and_then(|f| f.trailing_pe.as_ref().and_then(|v| v.raw)))
            .or_else(|| summary.and_then(|s| s.trailing_pe.as_ref().and_then(|v| v.raw))),
        forward_pe: stats
            .and_then(|s| s.forward_pe.as_ref().and_then(|v| v.raw))
            .or_else(|| fin.and_then(|f| f.forward_pe.as_ref().and_then(|v| v.raw)))
            .or_else(|| summary.and_then(|s| s.forward_pe.as_ref().and_then(|v| v.raw))),
        price_to_book: stats
            .and_then(|s| s.price_to_book.as_ref().and_then(|v| v.raw))
            .or_else(|| fin.and_then(|f| f.price_to_book.as_ref().and_then(|v| v.raw)))
            .or_else(|| summary.and_then(|s| s.price_to_book.as_ref().and_then(|v| v.raw))),
        return_on_equity: fin.and_then(|f| f.return_on_equity.as_ref().and_then(|v| v.raw)),
        profit_margins: fin.and_then(|f| f.profit_margins.as_ref().and_then(|v| v.raw)),
        return_on_assets: fin.and_then(|f| f.return_on_assets.as_ref().and_then(|v| v.raw)),
        revenue_growth: fin.and_then(|f| f.revenue_growth.as_ref().and_then(|v| v.raw)),
        earnings_growth: stats
            .and_then(|s| s.earnings_growth.as_ref().and_then(|v| v.raw))
            .or_else(|| fin.and_then(|f| f.earnings_growth.as_ref().and_then(|v| v.raw))),
        debt_to_equity: fin.and_then(|f| f.debt_to_equity.as_ref().and_then(|v| v.raw)),
        current_ratio: fin.and_then(|f| f.current_ratio.as_ref().and_then(|v| v.raw)),
        enterprise_value: stats
            .and_then(|s| s.enterprise_value.as_ref().and_then(|v| v.raw))
            .or_else(|| fin.and_then(|f| f.enterprise_value.as_ref().and_then(|v| v.raw))),
        ebitda: fin
            .and_then(|f| f.ebitda.as_ref().and_then(|v| v.raw))
            .or_else(|| stats.and_then(|s| s.ebitda.as_ref().and_then(|v| v.raw))),
        market_cap: fin
            .and_then(|f| f.market_cap.as_ref().and_then(|v| v.raw))
            .or_else(|| stats.and_then(|s| s.market_cap.as_ref().and_then(|v| v.raw)))
            .or_else(|| {
                summary
                    .and_then(|s| s.market_cap.as_ref().and_then(|v| v.raw))
                    .map(|n| n.round() as u64)
            }),
    })
}

/// Prices are IDR; anything non-finite, negative, or above this is not a
/// real quote, and treating it as missing keeps arithmetic from overflowing.
const MAX_PRICE: f64 = 1e12;

fn round_price(value: f64) -> Option<i64> {
    (value.is_finite() && (0.0..=MAX_PRICE).contains(&value)).then(|| value.round() as i64)
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

#[cfg(test)]
mod tests {
    use super::previous_close;

    /// A stated close that matches Yahoo's percentage stands; a stale one
    /// (the index on 2026-11-13) gives way to the close the percentage implies.
    #[test]
    fn a_stale_previous_close_gives_way_to_the_change_percent() {
        assert_eq!(
            previous_close(5500.0, Some(5475.0), Some(0.457)),
            Some(5475.0)
        );
        let index = previous_close(5344.406, Some(5115.04), Some(1.783)).unwrap();
        assert!((index - 5250.79).abs() < 0.5, "{index}");
        assert_eq!(previous_close(5500.0, Some(5475.0), None), Some(5475.0));
        assert_eq!(previous_close(5500.0, None, None), None);
        let derived = previous_close(5500.0, None, Some(0.457)).unwrap();
        assert!((derived - 5475.0).abs() < 0.5, "{derived}");
    }
}
