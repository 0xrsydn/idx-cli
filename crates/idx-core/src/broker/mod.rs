//! Broker summaries: which IDX brokers bought and sold a stock over a date
//! range (IPOT, end of day). See docs/ARCHITECTURE.md.

pub mod types;

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::api::{broker_provider, today_wib};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{BrokerBoard, BrokerInvestor, BrokerQuery, BrokerSummary};

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
    /// The broker summary for `symbol` over `query`'s range. An empty
    /// summary means no trades (weekend, holiday, suspension).
    pub fn summary(
        &self,
        symbol: &str,
        query: &BrokerQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<BrokerSummary>, IdxError> {
        let (resolved, code) = self.idx_code(symbol)?;
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
            |raw| {
                let filtered =
                    (query.investor, query.board) != (BrokerInvestor::All, BrokerBoard::All);
                let mut summary = BrokerSummary {
                    symbol: resolved.clone(),
                    from: raw.from.unwrap_or(query.from),
                    to: raw.to.unwrap_or(query.to),
                    investor: query.investor,
                    board: query.board,
                    buyers: raw.buyers,
                    sellers: raw.sellers,
                    total_value: raw.total_value,
                    total_lots: raw.total_lots,
                    average_price: raw.average_price,
                    foreign_net_value: raw.foreign_net_value.filter(|_| !filtered),
                    rounded: raw.rounded,
                };
                for flow in summary.buyers.iter_mut().chain(summary.sellers.iter_mut()) {
                    flow.name = broker_name(&flow.code).map(str::to_string);
                }
                Ok(summary)
            },
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
        let (resolved, _) = self.idx_code(symbol)?;
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

    /// The resolved symbol and the bare IDX code IPOT takes.
    fn idx_code(&self, symbol: &str) -> Result<(String, String), IdxError> {
        let resolved = self.idx.resolve(symbol)?;
        match resolved.strip_suffix(".JK") {
            Some(code) if !code.is_empty() => Ok((resolved.clone(), code.to_string())),
            _ => Err(IdxError::Unsupported(format!(
                "{resolved}: broker summaries cover IDX (.JK) stocks only"
            ))),
        }
    }
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
