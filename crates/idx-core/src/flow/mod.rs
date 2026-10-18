//! Foreign flow: what foreign investors bought and sold of a stock, session
//! by session, from IDX's daily trading summary (end of day). One request
//! covers every stock for one session and is cached by date.

pub mod types;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};

use crate::api::{today_wib, trading_summary_provider};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{FlowRange, ForeignFlow, ForeignFlowDay, StockDay};

const SOURCE: &str = "idx";
/// Most sessions `FlowRange::Latest` reads, one request each.
pub const MAX_SESSIONS: usize = 60;
/// Longest `FlowRange::Dates` range, calendar days.
pub const MAX_RANGE_DAYS: i64 = 92;
/// A past session no longer changes.
const TTL_CLOSED_SECS: u64 = 30 * 24 * 3600;
const TTL_TODAY_SECS: u64 = 5 * 60;

pub struct Flow<'a> {
    pub(crate) idx: &'a Idx,
}

impl Flow<'_> {
    /// Today in WIB.
    pub fn today(&self) -> NaiveDate {
        today_wib()
    }

    /// Foreign buying and selling of `symbol` over `range`.
    pub fn foreign(
        &self,
        symbol: &str,
        range: FlowRange,
        policy: CachePolicy,
    ) -> Result<Fetched<ForeignFlow>, IdxError> {
        let (resolved, code) = self.idx.idx_code(symbol, "foreign flow")?;
        let today = today_wib();
        let (dates, wanted, skip_misses) = match range {
            FlowRange::Latest { sessions } => {
                if !(1..=MAX_SESSIONS).contains(&sessions) {
                    return Err(IdxError::InvalidInput(format!(
                        "days must be 1 to {MAX_SESSIONS} (got {sessions})"
                    )));
                }
                let lookback = 2 * sessions as i64 + 10;
                let dates: Vec<NaiveDate> = (0..lookback)
                    .map(|back| today - Duration::days(back))
                    .collect();
                // Offline, an uncached day says nothing; look further back.
                (dates, sessions, policy == CachePolicy::Offline)
            }
            FlowRange::Dates { from, to } => {
                check_range(from, to, today)?;
                let dates = (0..=(to - from).num_days())
                    .rev()
                    .map(|offset| from + Duration::days(offset))
                    .collect();
                (dates, usize::MAX, false)
            }
        };

        let mut rows = Vec::new();
        let mut sessions = 0;
        let mut fetched_at: Option<DateTime<Utc>> = None;
        let mut stale = false;
        for date in dates {
            if sessions == wanted {
                break;
            }
            if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                continue;
            }
            let day = match self.session(date, today, policy) {
                Ok(day) => day,
                Err(IdxError::CacheMiss(_)) if skip_misses => continue,
                Err(err) => return Err(err),
            };
            if day.data.is_empty() {
                continue;
            }
            sessions += 1;
            fetched_at = Some(fetched_at.map_or(day.fetched_at, |at| at.min(day.fetched_at)));
            stale |= day.stale;
            // A stock missing from a session did not trade (suspension).
            if let Some(row) = day.data.into_iter().find(|row| row.code == code) {
                rows.push(row);
            }
        }
        if sessions == 0 {
            return Err(if policy == CachePolicy::Offline {
                IdxError::CacheMiss(format!("idx-stocksummary/{resolved} (no cached session)"))
            } else {
                IdxError::NoMarketData(format!("{resolved}: no IDX sessions in range"))
            });
        }
        rows.reverse();
        let flow = foreign_flow(&resolved, &rows).ok_or(IdxError::SymbolNotFound(code))?;
        Ok(Fetched {
            data: flow,
            source: SOURCE,
            fetched_at: fetched_at.unwrap_or_else(Utc::now),
            stale,
        })
    }

    /// Every stock's summary for one session; empty without trading.
    fn session(
        &self,
        date: NaiveDate,
        today: NaiveDate,
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<StockDay>>, IdxError> {
        let provider = trading_summary_provider();
        let key = date.to_string();
        cached_fetch(
            &self.idx.cache,
            CacheSpec {
                bucket: "idx-stocksummary",
                key: &key,
                ttl_secs: if date < today {
                    TTL_CLOSED_SECS
                } else {
                    TTL_TODAY_SECS
                },
                subject: &key,
                source: SOURCE,
            },
            policy,
            || provider.stock_summary(date),
            Ok,
        )
    }
}

fn check_range(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Result<(), IdxError> {
    if from > to {
        return Err(IdxError::InvalidInput(format!(
            "--from {from} is after --to {to}"
        )));
    }
    if to > today {
        return Err(IdxError::InvalidInput(format!(
            "{to} is in the future (today is {today} in WIB)"
        )));
    }
    if (to - from).num_days() >= MAX_RANGE_DAYS {
        return Err(IdxError::InvalidInput(format!(
            "a foreign flow covers at most {MAX_RANGE_DAYS} days"
        )));
    }
    Ok(())
}

/// `part` in basis points of `whole`, rounded; `None` without a positive whole.
fn bps(part: i64, whole: i64) -> Option<i64> {
    (whole > 0).then(|| (part as f64 * 10_000.0 / whole as f64).round() as i64)
}

fn flow_day(row: &StockDay) -> ForeignFlowDay {
    let foreign_net = row
        .foreign_buy
        .zip(row.foreign_sell)
        .map(|(buy, sell)| buy - sell);
    let foreign_net_value = match (foreign_net, row.value, row.volume) {
        (Some(net), Some(value), Some(volume)) if volume > 0 => {
            i64::try_from(i128::from(net) * i128::from(value) / i128::from(volume)).ok()
        }
        _ => None,
    };
    ForeignFlowDay {
        date: row.date,
        close: row.close,
        change_bps: row
            .close
            .zip(row.previous)
            .and_then(|(close, previous)| bps(close - previous, previous)),
        volume: row.volume,
        value: row.value,
        foreign_buy: row.foreign_buy,
        foreign_sell: row.foreign_sell,
        foreign_net,
        foreign_net_value,
        // `value` is the regular board only; the share is of all trading.
        non_regular_bps: row
            .non_regular_value
            .zip(row.value)
            .and_then(|(part, regular)| bps(part, part.checked_add(regular)?)),
    }
}

fn sum(days: &[ForeignFlowDay], field: fn(&ForeignFlowDay) -> Option<i64>) -> Option<i64> {
    days.iter()
        .try_fold(0_i64, |total, day| total.checked_add(field(day)?))
}

/// The flow over `rows` (one stock, oldest first); `None` without rows.
pub fn foreign_flow(symbol: &str, rows: &[StockDay]) -> Option<ForeignFlow> {
    let (first, last) = (rows.first()?, rows.last()?);
    let days: Vec<ForeignFlowDay> = rows.iter().map(flow_day).collect();
    let foreign_share_bps = match (
        sum(&days, |day| day.foreign_buy),
        sum(&days, |day| day.foreign_sell),
        sum(&days, |day| day.volume),
    ) {
        (Some(buy), Some(sell), Some(volume)) => bps(buy + sell, 2 * volume),
        _ => None,
    };
    Some(ForeignFlow {
        symbol: symbol.to_string(),
        from: first.date,
        to: last.date,
        foreign_net: sum(&days, |day| day.foreign_net),
        foreign_net_value: sum(&days, |day| day.foreign_net_value),
        foreign_share_bps,
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(day: u32, close: i64, buy: i64, sell: Option<i64>) -> StockDay {
        StockDay {
            code: "TEST".to_string(),
            date: NaiveDate::from_ymd_opt(2026, 10, day).unwrap(),
            previous: Some(1000),
            high: None,
            low: None,
            close: Some(close),
            volume: Some(1_000),
            value: Some(1_100_000),
            frequency: None,
            foreign_buy: Some(buy),
            foreign_sell: sell,
            non_regular_volume: None,
            non_regular_value: Some(110_000),
        }
    }

    #[test]
    fn test_foreign_flow_sums_sessions_and_estimates_value() {
        let flow = foreign_flow(
            "TEST.JK",
            &[row(15, 1100, 600, Some(200)), row(16, 950, 100, Some(300))],
        )
        .unwrap();
        let first = &flow.days[0];
        assert_eq!(
            (first.foreign_net, first.change_bps),
            (Some(400), Some(1000))
        );
        // 400 shares at 1,100 (value over volume).
        assert_eq!(first.foreign_net_value, Some(440_000));
        // 110,000 of 1,100,000 regular plus 110,000 non-regular.
        assert_eq!(first.non_regular_bps, Some(909));
        assert_eq!(flow.days[1].change_bps, Some(-500));
        assert_eq!(
            (flow.foreign_net, flow.foreign_net_value),
            (Some(200), Some(220_000))
        );
        // (700 + 500) / (2 * 2,000).
        assert_eq!(flow.foreign_share_bps, Some(3000));
        assert_eq!((flow.from.day(), flow.to.day()), (15, 16));
    }

    #[test]
    fn test_foreign_flow_never_counts_a_missing_figure_as_zero() {
        let flow = foreign_flow(
            "TEST.JK",
            &[row(15, 1100, 600, Some(200)), row(16, 950, 100, None)],
        )
        .unwrap();
        assert_eq!(flow.days[1].foreign_net, None);
        assert_eq!((flow.foreign_net, flow.foreign_share_bps), (None, None));
        assert!(foreign_flow("TEST.JK", &[]).is_none());
    }

    #[test]
    fn test_non_regular_share_is_of_all_trading() {
        let raw = include_str!("../../tests/fixtures/idx_stocksummary_20261014.json");
        let date = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let rows = crate::api::idx::parse_stock_summary(raw, date).unwrap();
        let bbri: Vec<StockDay> = rows.into_iter().filter(|row| row.code == "BBRI").collect();
        let flow = foreign_flow("BBRI.JK", &bbri).unwrap();
        // 172.3B non-regular against 338.5B regular: 33.74%, not 50.91%.
        assert_eq!(flow.days[0].non_regular_bps, Some(3374));
    }

    #[test]
    fn test_check_range_bounds() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let today = day(18);
        assert!(check_range(day(1), day(16), today).is_ok());
        assert!(check_range(day(16), day(12), today).is_err());
        assert!(check_range(day(16), day(19), today).is_err());
        let long_ago = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        assert!(check_range(long_ago, day(16), today).is_err());
    }
}
