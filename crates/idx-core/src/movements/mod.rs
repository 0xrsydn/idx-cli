//! Shareholder movements from the user's Stockbit session: reported changes
//! in insiders' and major holders' stakes (IDX insider disclosures and KSEI's
//! 5% holder changes), one stock's or the market's, newest first. Each page
//! is 50 rows and one request (paced 1 s apart). A query's pages are read
//! together and cached together for an hour, so they come from one moment
//! of a feed that gains rows at the top.
//!
//! These are the reports as filed. Whether they predict anything is not
//! claimed here: share plans read as a cluster of buys at one price, and a
//! row appears days after the trade (see `docs/STOCKBIT.md`).

pub mod types;

use std::cell::RefCell;

use chrono::Utc;

use crate::api::{ShareholderMovementProvider, shareholder_movement_provider, stockbit};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use crate::runtime;
use types::{MAX_PAGES, MovementList, MovementQuery};

/// The feed gains rows through the day; an hour keeps a session's repeated
/// queries cheap without hiding a day's filings for long.
const TTL_SECS: u64 = 3600;
const BUCKET: &str = "stockbit-movements";

pub struct Movements<'a> {
    pub(crate) idx: &'a Idx,
}

impl Movements<'_> {
    /// Up to `query.pages` pages of the feed (at most `MAX_PAGES`), newest
    /// first; `more` says whether rows remain past them.
    ///
    /// When a page after the first fails and no cached answer is at hand,
    /// the rows read before it are returned with `incomplete` saying why,
    /// and are not cached.
    pub fn list(
        &self,
        query: &MovementQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<MovementList>, IdxError> {
        let mut query = query.clone();
        query.pages = query.pages.clamp(1, MAX_PAGES);
        if let Some(symbol) = &query.symbol {
            let (resolved, _) = self.idx.idx_code(symbol, "shareholder movements")?;
            query.symbol = Some(resolved);
        }
        let provider = shareholder_movement_provider();
        let subject = query
            .symbol
            .clone()
            .unwrap_or_else(|| "shareholder movements".to_string());
        let key = cache_key(&query);
        let partial = RefCell::new(None);
        let result = cached_fetch(
            &self.idx.cache,
            CacheSpec {
                bucket: BUCKET,
                key: &key,
                ttl_secs: TTL_SECS,
                subject: &subject,
                source: stockbit::SOURCE,
            },
            policy,
            || read_pages(provider.as_ref(), &query, &partial),
            Ok,
        );
        match (result, partial.into_inner()) {
            (Err(err), Some(mut list)) => {
                runtime::warn(format!(
                    "{err}; showing the {} rows read before it",
                    list.movements.len()
                ));
                list.incomplete = Some(err.to_string());
                Ok(Fetched {
                    data: list,
                    source: stockbit::SOURCE,
                    fetched_at: Utc::now(),
                    stale: false,
                })
            }
            (result, _) => result,
        }
    }
}

/// Reads the query's pages in turn. A failure after the first page leaves
/// the rows read so far in `partial` and fails, so they are not cached.
fn read_pages(
    provider: &dyn ShareholderMovementProvider,
    query: &MovementQuery,
    partial: &RefCell<Option<MovementList>>,
) -> Result<MovementList, IdxError> {
    let mut list = MovementList::default();
    for page in 1..=query.pages {
        match provider.shareholder_movements(query, page) {
            Ok(part) => {
                list.movements.extend(part.movements);
                list.more = part.more;
                if !part.more {
                    break;
                }
            }
            Err(err) if page > 1 => {
                list.more = true;
                *partial.borrow_mut() = Some(list);
                return Err(err);
            }
            Err(err) => return Err(err),
        }
    }
    Ok(list)
}

/// Every filter and the page count, so no two queries share an entry.
fn cache_key(query: &MovementQuery) -> String {
    let opt = |value: Option<String>| value.unwrap_or_else(|| "-".to_string());
    format!(
        "{}:{}:{}:{}:{}:{}:{}p",
        query.symbol.as_deref().unwrap_or("market"),
        opt(query.from.map(|d| d.to_string())),
        opt(query.to.map(|d| d.to_string())),
        query.holder_id.as_deref().unwrap_or("-"),
        query.action.map(|a| a.as_str()).unwrap_or("-"),
        query.source.map(|s| s.as_str()).unwrap_or("-"),
        query.pages,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{ActionFilter, MovementSource};

    #[test]
    fn cache_keys_differ_by_every_filter() {
        let base = MovementQuery::from_args(None, None, None, None, 1).unwrap();
        let mut two = base.clone();
        two.pages = 2;
        let mut keys = vec![cache_key(&base), cache_key(&two)];
        let mut q = base.clone();
        q.symbol = Some("BBCA.JK".to_string());
        keys.push(cache_key(&q));
        let mut q = base.clone();
        q.from = chrono::NaiveDate::from_ymd_opt(2026, 10, 1);
        keys.push(cache_key(&q));
        let mut q = base.clone();
        q.to = chrono::NaiveDate::from_ymd_opt(2026, 10, 1);
        keys.push(cache_key(&q));
        let mut q = base.clone();
        q.holder_id = Some("14074".to_string());
        keys.push(cache_key(&q));
        let mut q = base.clone();
        q.action = Some(ActionFilter::Sell);
        keys.push(cache_key(&q));
        let mut q = base;
        q.source = Some(MovementSource::Ksei);
        keys.push(cache_key(&q));
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "{keys:?}");
    }
}
