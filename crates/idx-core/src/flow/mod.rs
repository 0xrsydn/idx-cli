//! Foreign flow: what foreign investors bought and sold of a stock, session
//! by session (end of day). IPOT (the default) needs two plain requests per
//! session; IDX's trading summary covers every stock in one request per
//! session, cached by date, but needs curl-impersonate.

pub mod types;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};

use crate::api::{today_wib, trading_summary_provider};
use crate::broker::types::{BrokerBoard, BrokerInvestor, BrokerQuery, BrokerSummary};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{FlowRange, FlowSource, ForeignFlow, ForeignFlowDay, StockDay};

/// Most sessions `FlowRange::Latest` reads.
pub const MAX_SESSIONS: usize = 60;
/// Longest `FlowRange::Dates` range, calendar days.
pub const MAX_RANGE_DAYS: i64 = 92;
/// A past session no longer changes.
const TTL_CLOSED_SECS: u64 = 30 * 24 * 3600;
const TTL_TODAY_SECS: u64 = 5 * 60;

pub struct Flow<'a> {
    pub(crate) idx: &'a Idx,
}

/// What one weekday held for the stock.
enum Day {
    /// The market (or, for IPOT, the stock) did not trade.
    NoSession,
    /// The market traded without the stock (a suspension).
    Absent,
    Traded {
        day: Box<ForeignFlowDay>,
        rounded: bool,
    },
}

struct Fetch {
    day: Day,
    fetched_at: DateTime<Utc>,
    stale: bool,
}

impl Flow<'_> {
    /// Today in WIB.
    pub fn today(&self) -> NaiveDate {
        today_wib()
    }

    /// Foreign buying and selling of `symbol` over `range`, from `source`.
    pub fn foreign(
        &self,
        symbol: &str,
        range: FlowRange,
        source: FlowSource,
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

        let mut days = Vec::new();
        let mut sessions = 0;
        let mut rounded = false;
        let mut fetched_at: Option<DateTime<Utc>> = None;
        let mut stale = false;
        for date in dates {
            if sessions == wanted {
                break;
            }
            if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                continue;
            }
            let fetch = match source {
                FlowSource::Ipot => self.ipot_day(&resolved, date, policy),
                FlowSource::Idx => self.idx_day(&code, date, today, policy),
            };
            let fetch = match fetch {
                Ok(fetch) => fetch,
                Err(IdxError::CacheMiss(_)) if skip_misses => continue,
                Err(err) => return Err(err),
            };
            match fetch.day {
                Day::NoSession => continue,
                Day::Absent => {}
                Day::Traded {
                    day,
                    rounded: day_rounded,
                } => {
                    days.push(*day);
                    rounded |= day_rounded;
                }
            }
            sessions += 1;
            fetched_at = Some(fetched_at.map_or(fetch.fetched_at, |at| at.min(fetch.fetched_at)));
            stale |= fetch.stale;
        }
        if sessions == 0 {
            return Err(if policy == CachePolicy::Offline {
                IdxError::CacheMiss(format!(
                    "{}-flow/{resolved} (no cached session)",
                    source.as_str()
                ))
            } else {
                IdxError::NoMarketData(format!("{resolved}: no sessions in range"))
            });
        }
        days.reverse();
        let flow =
            foreign_flow(&resolved, source, days, rounded).ok_or(IdxError::SymbolNotFound(code))?;
        Ok(Fetched {
            data: flow,
            source: source.as_str(),
            fetched_at: fetched_at.unwrap_or_else(Utc::now),
            stale,
        })
    }

    /// One session from IPOT: the all-investor summary for the value traded,
    /// the foreign one for foreign buying, selling and net. Both go through
    /// the broker summary cache.
    fn ipot_day(
        &self,
        resolved: &str,
        date: NaiveDate,
        policy: CachePolicy,
    ) -> Result<Fetch, IdxError> {
        let broker = self.idx.broker();
        let all_query = BrokerQuery {
            from: date,
            to: date,
            investor: BrokerInvestor::All,
            board: BrokerBoard::All,
        };
        let all = broker.summary(resolved, &all_query, policy)?;
        if all.data.is_empty() {
            return Ok(Fetch {
                day: Day::NoSession,
                fetched_at: all.fetched_at,
                stale: all.stale,
            });
        }
        let foreign_query = BrokerQuery {
            investor: BrokerInvestor::Foreign,
            ..all_query
        };
        let foreign = broker.summary(resolved, &foreign_query, policy)?;
        Ok(Fetch {
            day: Day::Traded {
                day: Box::new(ipot_flow_day(date, &all.data, &foreign.data)),
                rounded: all.data.rounded || foreign.data.rounded,
            },
            fetched_at: all.fetched_at.min(foreign.fetched_at),
            stale: all.stale || foreign.stale,
        })
    }

    /// One session from IDX's trading summary of every stock.
    fn idx_day(
        &self,
        code: &str,
        date: NaiveDate,
        today: NaiveDate,
        policy: CachePolicy,
    ) -> Result<Fetch, IdxError> {
        let session = self.session(date, today, policy)?;
        let day = if session.data.is_empty() {
            Day::NoSession
        } else {
            match session.data.iter().find(|row| row.code == code) {
                Some(row) => Day::Traded {
                    day: Box::new(idx_flow_day(row)),
                    rounded: false,
                },
                None => Day::Absent,
            }
        };
        Ok(Fetch {
            day,
            fetched_at: session.fetched_at,
            stale: session.stale,
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
                source: FlowSource::Idx.as_str(),
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

/// `shares` at the session's average price (value over volume), whole IDR.
fn at_average(shares: Option<i64>, value: Option<i64>, volume: Option<i64>) -> Option<i64> {
    match (shares, value, volume) {
        (Some(shares), Some(value), Some(volume)) if volume > 0 => {
            i64::try_from(i128::from(shares) * i128::from(value) / i128::from(volume)).ok()
        }
        _ => None,
    }
}

/// Foreign buying plus selling over twice `total`.
fn share(buy: Option<i64>, sell: Option<i64>, total: Option<i64>) -> Option<i64> {
    let traded = buy?.checked_add(sell?)?;
    bps(traded, total?.checked_mul(2)?)
}

/// One IPOT session: the value traded from the all-investor summary, the
/// foreign figures from the foreign one (`buy_value` is its footer,
/// `sell_value` that less the foreign net).
pub fn ipot_flow_day(
    date: NaiveDate,
    all: &BrokerSummary,
    foreign: &BrokerSummary,
) -> ForeignFlowDay {
    let foreign_net_value = foreign.foreign_net_value.or(all.foreign_net_value);
    ForeignFlowDay {
        date,
        close: None,
        change_bps: None,
        volume: all.total_lots.and_then(|lots| lots.checked_mul(100)),
        value: all.total_value,
        foreign_buy: None,
        foreign_sell: None,
        foreign_net: None,
        foreign_buy_value: foreign.buy_value,
        foreign_sell_value: foreign.sell_value,
        foreign_net_value,
        foreign_share_bps: share(foreign.buy_value, foreign.sell_value, all.total_value),
        non_regular_bps: None,
    }
}

/// One IDX session for one stock.
pub fn idx_flow_day(row: &StockDay) -> ForeignFlowDay {
    let foreign_net = row
        .foreign_buy
        .zip(row.foreign_sell)
        .map(|(buy, sell)| buy - sell);
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
        foreign_buy_value: at_average(row.foreign_buy, row.value, row.volume),
        foreign_sell_value: at_average(row.foreign_sell, row.value, row.volume),
        foreign_net_value: at_average(foreign_net, row.value, row.volume),
        foreign_share_bps: share(row.foreign_buy, row.foreign_sell, row.volume),
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

/// The flow over `days` (one stock, oldest first); `None` without days.
pub fn foreign_flow(
    symbol: &str,
    source: FlowSource,
    days: Vec<ForeignFlowDay>,
    rounded: bool,
) -> Option<ForeignFlow> {
    let (from, to) = (days.first()?.date, days.last()?.date);
    // IDX has exact shares; IPOT has values.
    let foreign_share_bps = match source {
        FlowSource::Idx => share(
            sum(&days, |day| day.foreign_buy),
            sum(&days, |day| day.foreign_sell),
            sum(&days, |day| day.volume),
        ),
        FlowSource::Ipot => share(
            sum(&days, |day| day.foreign_buy_value),
            sum(&days, |day| day.foreign_sell_value),
            sum(&days, |day| day.value),
        ),
    };
    Some(ForeignFlow {
        symbol: symbol.to_string(),
        source,
        from,
        to,
        foreign_net: sum(&days, |day| day.foreign_net),
        foreign_buy_value: sum(&days, |day| day.foreign_buy_value),
        foreign_sell_value: sum(&days, |day| day.foreign_sell_value),
        foreign_net_value: sum(&days, |day| day.foreign_net_value),
        foreign_share_bps,
        rounded,
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

    fn idx_flow(rows: &[StockDay]) -> Option<ForeignFlow> {
        let days = rows.iter().map(idx_flow_day).collect();
        foreign_flow("TEST.JK", FlowSource::Idx, days, false)
    }

    #[test]
    fn test_foreign_flow_sums_sessions_and_estimates_value() {
        let flow =
            idx_flow(&[row(15, 1100, 600, Some(200)), row(16, 950, 100, Some(300))]).unwrap();
        let first = &flow.days[0];
        assert_eq!(
            (first.foreign_net, first.change_bps),
            (Some(400), Some(1000))
        );
        // 400 shares at 1,100 (value over volume).
        assert_eq!(first.foreign_net_value, Some(440_000));
        assert_eq!(
            (first.foreign_buy_value, first.foreign_sell_value),
            (Some(660_000), Some(220_000))
        );
        assert_eq!(first.foreign_share_bps, Some(4000));
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
        let flow = idx_flow(&[row(15, 1100, 600, Some(200)), row(16, 950, 100, None)]).unwrap();
        assert_eq!(flow.days[1].foreign_net, None);
        assert_eq!((flow.foreign_net, flow.foreign_share_bps), (None, None));
        assert!(idx_flow(&[]).is_none());
    }

    #[test]
    fn test_non_regular_share_is_of_all_trading() {
        let raw = include_str!("../../tests/fixtures/idx_stocksummary_20261014.json");
        let date = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let rows = crate::api::idx::parse_stock_summary(raw, date).unwrap();
        let bbri: Vec<StockDay> = rows.into_iter().filter(|row| row.code == "BBRI").collect();
        let flow = idx_flow(&bbri).unwrap();
        // 172.3B non-regular against 338.5B regular: 33.74%, not 50.91%.
        assert_eq!(flow.days[0].non_regular_bps, Some(3374));
    }

    #[test]
    fn test_ipot_flow_day_reads_foreign_buying_and_selling_from_the_summaries() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let summary = |investor, name: &str| {
            let html = std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap();
            let raw = crate::api::ipot::summary_from_html("BBCA", &html).unwrap();
            let query = BrokerQuery {
                from: day,
                to: day,
                investor,
                board: BrokerBoard::All,
            };
            crate::broker::summary_from_raw("BBCA.JK", &query, raw)
        };
        let all = summary(BrokerInvestor::All, "ipot_brokersummary_bbca_20261016.html");
        let foreign = summary(
            BrokerInvestor::Foreign,
            "ipot_brokersummary_bbca_20261016_foreign_all.html",
        );
        let flow = ipot_flow_day(day, &all, &foreign);
        assert_eq!(flow.value, Some(6_600_000_000_000));
        assert_eq!(flow.volume, Some(1_260_000_000));
        assert_eq!(
            (
                flow.foreign_buy_value,
                flow.foreign_sell_value,
                flow.foreign_net_value
            ),
            (
                Some(1_700_000_000_000),
                Some(4_000_000_000_000),
                Some(-2_300_000_000_000)
            )
        );
        // (1.7 T + 4.0 T) / (2 * 6.6 T).
        assert_eq!(flow.foreign_share_bps, Some(4318));
        assert_eq!((flow.close, flow.foreign_net), (None, None));
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
