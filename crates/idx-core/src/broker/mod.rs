//! Broker summaries: which IDX brokers bought and sold a stock over a date
//! range (IPOT, end of day). See docs/ARCHITECTURE.md.

pub mod types;

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::api::ipot::BrokerSummaryRaw;
use crate::api::{broker_provider, today_wib};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{BrokerBoard, BrokerInvestor, BrokerQuery, BrokerRange, BrokerSummary};

const SOURCE: &str = "ipot";
/// Longest range one summary may cover.
pub const MAX_RANGE_DAYS: i64 = 366;
/// Calendar days searched back for the latest session with trades.
const LATEST_LOOKBACK_DAYS: i64 = 10;
/// A range that ended before today no longer changes.
const TTL_CLOSED_SECS: u64 = 7 * 24 * 3600;
/// Today's summary moves with every trade.
const TTL_TODAY_SECS: u64 = 5 * 60;

const CODES: &str = include_str!("codes.tsv");

/// Member name for an IDX broker code such as `CC`; `None` when the code is
/// not on the bundled member list.
pub fn broker_name(code: &str) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            CODES
                .lines()
                .filter(|line| !line.starts_with('#'))
                .filter_map(|line| line.split_once('\t'))
                .collect()
        })
        .get(code.trim().to_ascii_uppercase().as_str())
        .copied()
}

pub struct Broker<'a> {
    pub(crate) idx: &'a Idx,
}

impl Broker<'_> {
    /// Today in WIB, the date `BrokerRange::from_args` defaults `to` to.
    pub fn today(&self) -> NaiveDate {
        today_wib()
    }

    /// The summary a frontend asked for: the latest session or a range.
    pub fn fetch(
        &self,
        symbol: &str,
        range: BrokerRange,
        investor: BrokerInvestor,
        board: BrokerBoard,
        policy: CachePolicy,
    ) -> Result<Fetched<BrokerSummary>, IdxError> {
        match range {
            BrokerRange::Latest => self.latest(symbol, investor, board, policy),
            BrokerRange::Dates { from, to } => self.summary(
                symbol,
                &BrokerQuery {
                    from,
                    to,
                    investor,
                    board,
                },
                policy,
            ),
        }
    }

    /// The broker summary for `symbol` over `query`'s range. An empty
    /// summary means no trades (weekend, holiday, suspension).
    pub fn summary(
        &self,
        symbol: &str,
        query: &BrokerQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<BrokerSummary>, IdxError> {
        let (resolved, code) = self.idx.idx_code(symbol, "a broker summary")?;
        let today = today_wib();
        check_range(query, today)?;
        let key = format!(
            "{code}-{}-{}-{}-{}",
            query.from,
            query.to,
            query.investor.as_str(),
            query.board.as_str()
        );
        let provider = broker_provider();
        cached_fetch(
            &self.idx.cache,
            CacheSpec {
                bucket: "ipot-broker",
                key: &key,
                ttl_secs: if query.to < today {
                    TTL_CLOSED_SECS
                } else {
                    TTL_TODAY_SECS
                },
                subject: &resolved,
                source: SOURCE,
            },
            policy,
            || provider.broker_summary(&code, query),
            |raw| Ok(summary_from_raw(&resolved, query, raw)),
        )
    }

    /// The summary of the latest session with trades, searching back from
    /// today (WIB) over weekdays.
    pub fn latest(
        &self,
        symbol: &str,
        investor: BrokerInvestor,
        board: BrokerBoard,
        policy: CachePolicy,
    ) -> Result<Fetched<BrokerSummary>, IdxError> {
        let (resolved, _) = self.idx.idx_code(symbol, "a broker summary")?;
        let today = today_wib();
        for back in 0..LATEST_LOOKBACK_DAYS {
            let day = today - Duration::days(back);
            if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                continue;
            }
            let query = BrokerQuery {
                from: day,
                to: day,
                investor,
                board,
            };
            match self.summary(symbol, &query, policy) {
                Ok(fetched) if !fetched.data.is_empty() => return Ok(fetched),
                Ok(_) => {}
                // Offline, an uncached day says nothing; look further back.
                Err(IdxError::CacheMiss(_)) if policy == CachePolicy::Offline => {}
                Err(err) => return Err(err),
            }
        }
        if policy == CachePolicy::Offline {
            return Err(IdxError::CacheMiss(format!(
                "ipot-broker/{resolved} (no cached session in the last {LATEST_LOOKBACK_DAYS} days)"
            )));
        }
        Err(IdxError::NoMarketData(format!(
            "{resolved}: no trades in the last {LATEST_LOOKBACK_DAYS} days"
        )))
    }
}

/// The public summary of one parsed page. With one investor side selected,
/// the page's footer covers only part of the trades the lists show (foreign
/// buying, not foreign selling), so no totals are given; `F. NVal` ignores
/// both filters, so the foreign net goes with any filter.
fn summary_from_raw(symbol: &str, query: &BrokerQuery, raw: BrokerSummaryRaw) -> BrokerSummary {
    let one_side = query.investor != BrokerInvestor::All;
    let filtered = one_side || query.board != BrokerBoard::All;
    let mut summary = BrokerSummary {
        symbol: symbol.to_string(),
        from: raw.from.unwrap_or(query.from),
        to: raw.to.unwrap_or(query.to),
        investor: query.investor,
        board: query.board,
        buyers: raw.buyers,
        sellers: raw.sellers,
        total_value: raw.total_value.filter(|_| !one_side),
        total_lots: raw.total_lots.filter(|_| !one_side),
        average_price: raw.average_price.filter(|_| !one_side),
        foreign_net_value: raw.foreign_net_value.filter(|_| !filtered),
        rounded: raw.rounded,
    };
    for flow in summary.buyers.iter_mut().chain(summary.sellers.iter_mut()) {
        flow.name = broker_name(&flow.code).map(str::to_string);
    }
    summary
}

fn check_range(query: &BrokerQuery, today: NaiveDate) -> Result<(), IdxError> {
    if query.from > query.to {
        return Err(IdxError::InvalidInput(format!(
            "--from {} is after --to {}",
            query.from, query.to
        )));
    }
    if query.to > today {
        return Err(IdxError::InvalidInput(format!(
            "{} is in the future (today is {today} in WIB)",
            query.to
        )));
    }
    if (query.to - query.from).num_days() >= MAX_RANGE_DAYS {
        return Err(IdxError::InvalidInput(format!(
            "a broker summary covers at most {MAX_RANGE_DAYS} days"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn query(from: NaiveDate, to: NaiveDate) -> BrokerQuery {
        BrokerQuery {
            from,
            to,
            investor: BrokerInvestor::All,
            board: BrokerBoard::All,
        }
    }

    #[test]
    fn test_broker_name_reads_the_bundled_member_list() {
        assert_eq!(broker_name("cc"), Some("MANDIRI SEKURITAS"));
        assert_eq!(broker_name("XL"), Some("STOCKBIT SEKURITAS DIGITAL"));
        assert_eq!(broker_name("??"), None);
    }

    #[test]
    fn test_summary_from_raw_drops_totals_that_do_not_cover_the_lists() {
        let day = date(2026, 10, 16);
        let read = |name: &str| {
            let html = std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap();
            crate::api::ipot::summary_from_html("BBCA", &html).unwrap()
        };
        let all = summary_from_raw(
            "BBCA.JK",
            &query(day, day),
            read("ipot_brokersummary_bbca_20261016.html"),
        );
        assert_eq!(all.total_value, Some(6_600_000_000_000));
        assert_eq!(all.foreign_net_value, Some(-2_300_000_000_000));
        assert_eq!(all.sellers[0].name.as_deref(), Some("MANDIRI SEKURITAS"));

        // Foreign, regular board: the footer's 1.6 T is less than the 2.0 T
        // its top seller alone sold.
        let foreign = BrokerQuery {
            investor: BrokerInvestor::Foreign,
            board: BrokerBoard::Regular,
            ..query(day, day)
        };
        let raw = read("ipot_brokersummary_bbca_20261016_foreign_regular.html");
        assert!(raw.total_value < raw.sellers[0].value);
        let filtered = summary_from_raw("BBCA.JK", &foreign, raw);
        assert_eq!(
            (
                filtered.total_value,
                filtered.total_lots,
                filtered.average_price
            ),
            (None, None, None)
        );
        assert_eq!(filtered.foreign_net_value, None);
        assert_eq!(filtered.sellers[0].value, Some(2_000_000_000_000));
    }

    #[test]
    fn test_broker_range_from_args() {
        let today = date(2026, 10, 18);
        let range = |date, from, to| BrokerRange::from_args(date, from, to, today);
        assert_eq!(range(None, None, None).unwrap(), BrokerRange::Latest);
        assert_eq!(
            range(Some("2026-10-16"), None, None).unwrap(),
            BrokerRange::Dates {
                from: date(2026, 10, 16),
                to: date(2026, 10, 16)
            }
        );
        assert_eq!(
            range(None, Some("2026-10-12"), None).unwrap(),
            BrokerRange::Dates {
                from: date(2026, 10, 12),
                to: today
            }
        );
        for (d, f, t) in [
            (None, None, Some("2026-10-16")),
            (Some("2026-10-16"), Some("2026-10-12"), None),
            (Some("16/10/2026"), None, None),
            (None, Some("2026-10-12"), Some("tomorrow")),
        ] {
            assert!(
                matches!(range(d, f, t), Err(IdxError::InvalidInput(_))),
                "{d:?} {f:?} {t:?}"
            );
        }
    }

    #[test]
    fn test_check_range_bounds() {
        let today = date(2026, 10, 18);
        assert!(check_range(&query(date(2026, 10, 12), date(2026, 10, 16)), today).is_ok());
        assert!(check_range(&query(today, today), today).is_ok());
        assert!(check_range(&query(date(2025, 10, 16), date(2026, 10, 16)), today).is_ok());
        for bad in [
            query(date(2026, 10, 16), date(2026, 10, 12)),
            query(date(2026, 10, 19), date(2026, 10, 19)),
            query(date(2025, 10, 15), date(2026, 10, 16)),
        ] {
            assert!(
                matches!(check_range(&bad, today), Err(IdxError::InvalidInput(_))),
                "{bad:?}"
            );
        }
    }
}
