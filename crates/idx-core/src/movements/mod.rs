//! Shareholder movements from the user's Stockbit session: reported changes
//! in insiders' and major holders' stakes (IDX insider disclosures and KSEI's
//! 5% holder changes), one stock's or the market's, newest first. Each page
//! is 50 rows and one request (paced 1 s apart), cached for an hour.
//!
//! These are the reports as filed. Whether they predict anything is not
//! claimed here: share plans read as a cluster of buys at one price, and a
//! row appears days after the trade (see `docs/STOCKBIT.md`).

pub mod types;

use chrono::Utc;

use crate::api::{shareholder_movement_provider, stockbit};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{MovementList, MovementQuery};

/// The feed gains rows through the day; an hour keeps a session's repeated
/// queries cheap without hiding a day's filings for long.
const TTL_SECS: u64 = 3600;
const BUCKET: &str = "stockbit-movements";

pub struct Movements<'a> {
    pub(crate) idx: &'a Idx,
}

impl Movements<'_> {
    /// Up to `query.pages` pages of the feed, newest first; `more` says
    /// whether rows remain past them. The result is as old as its oldest page
    /// and stale if any page is.
    pub fn list(
        &self,
        query: &MovementQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<MovementList>, IdxError> {
        let mut query = query.clone();
        if let Some(symbol) = &query.symbol {
            let (resolved, _) = self.idx.idx_code(symbol, "shareholder movements")?;
            query.symbol = Some(resolved);
        }
        let provider = shareholder_movement_provider();
        let subject = query
            .symbol
            .clone()
            .unwrap_or_else(|| "shareholder movements".to_string());
        let mut list = Fetched {
            data: MovementList::default(),
            source: stockbit::SOURCE,
            fetched_at: Utc::now(),
            stale: false,
        };
        for page in 1..=query.pages.max(1) {
            let key = cache_key(&query, page);
            let part = cached_fetch(
                &self.idx.cache,
                CacheSpec {
                    bucket: BUCKET,
                    key: &key,
                    ttl_secs: TTL_SECS,
                    subject: &subject,
                    source: stockbit::SOURCE,
                },
                policy,
                || provider.shareholder_movements(&query, page),
                Ok,
            )?;
            list.fetched_at = list.fetched_at.min(part.fetched_at);
            list.stale |= part.stale;
            list.data.movements.extend(part.data.movements);
            list.data.more = part.data.more;
            if !part.data.more {
                break;
            }
        }
        Ok(list)
    }
}

/// Every filter and the page, so no two queries share an entry.
fn cache_key(query: &MovementQuery, page: u32) -> String {
    let opt = |value: Option<String>| value.unwrap_or_else(|| "-".to_string());
    format!(
        "{}:{}:{}:{}:{}:{}:p{page}",
        query.symbol.as_deref().unwrap_or("market"),
        opt(query.from.map(|d| d.to_string())),
        opt(query.to.map(|d| d.to_string())),
        query.holder_id.as_deref().unwrap_or("-"),
        query.action.map(|a| a.as_str()).unwrap_or("-"),
        query.source.map(|s| s.as_str()).unwrap_or("-"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{ActionFilter, MovementSource};

    #[test]
    fn cache_keys_differ_by_every_filter() {
        let base = MovementQuery::from_args(None, None, None, None, 1).unwrap();
        let mut keys = vec![cache_key(&base, 1), cache_key(&base, 2)];
        let mut q = base.clone();
        q.symbol = Some("BBCA.JK".to_string());
        keys.push(cache_key(&q, 1));
        let mut q = base.clone();
        q.from = chrono::NaiveDate::from_ymd_opt(2026, 10, 1);
        keys.push(cache_key(&q, 1));
        let mut q = base.clone();
        q.to = chrono::NaiveDate::from_ymd_opt(2026, 10, 1);
        keys.push(cache_key(&q, 1));
        let mut q = base.clone();
        q.holder_id = Some("14074".to_string());
        keys.push(cache_key(&q, 1));
        let mut q = base.clone();
        q.action = Some(ActionFilter::Sell);
        keys.push(cache_key(&q, 1));
        let mut q = base;
        q.source = Some(MovementSource::Ksei);
        keys.push(cache_key(&q, 1));
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "{keys:?}");
    }
}
