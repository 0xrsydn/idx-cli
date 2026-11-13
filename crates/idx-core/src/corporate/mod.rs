//! Corporate actions (cash and stock dividends, bonus shares, rights issues,
//! warrants, splits, shareholders' meetings, public exposes, IPOs, tender
//! offers) from the user's Stockbit session: one stock's history, or the
//! market's calendar. Stockbit serves one kind per request, so a query costs
//! one request per kind (paced 1 s apart), each cached for 6 hours.

pub mod types;

use chrono::{NaiveDate, Utc};

use crate::api::{corporate_action_provider, stockbit, today_wib};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::error::IdxError;
use types::{ActionKind, CalendarRange, CorporateAction};

/// Announcements change a few times a day at most.
const TTL_SECS: u64 = 6 * 3600;
const BUCKET: &str = "stockbit-corpaction";

pub struct Corporate<'a> {
    pub(crate) idx: &'a Idx,
}

impl Corporate<'_> {
    /// Today in WIB.
    pub fn today(&self) -> NaiveDate {
        today_wib()
    }

    /// One IDX stock's corporate actions of `kinds`, newest first.
    pub fn actions(
        &self,
        symbol: &str,
        kinds: &[ActionKind],
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<CorporateAction>>, IdxError> {
        let (resolved, _) = self.idx.idx_code(symbol, "corporate actions")?;
        let mut fetched = self.fetch(kinds, Some(&resolved), policy)?;
        // Stockbit ignores `symbol` for some kinds (IPOs, seen 2026-11-13)
        // and answers the market's list.
        fetched.data.retain(|action| action.symbol == resolved);
        fetched
            .data
            .sort_by(|a, b| b.date.cmp(&a.date).then(a.kind.cmp(&b.kind)));
        Ok(fetched)
    }

    /// The market's corporate actions of `kinds` dated within `range` (see
    /// `CorporateAction::date`), plus offers still open in it (see
    /// `CalendarRange::includes`), oldest first.
    pub fn calendar(
        &self,
        range: CalendarRange,
        kinds: &[ActionKind],
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<CorporateAction>>, IdxError> {
        let fetched = self.fetch(kinds, None, policy)?;
        Ok(fetched.map(|mut actions| {
            actions.retain(|a| range.includes(a));
            actions.sort_by(|a, b| (a.date, &a.symbol, a.kind).cmp(&(b.date, &b.symbol, b.kind)));
            actions
        }))
    }

    /// Every kind in turn, through the cache; the result is as old as its
    /// oldest part and stale if any part is.
    fn fetch(
        &self,
        kinds: &[ActionKind],
        resolved: Option<&str>,
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<CorporateAction>>, IdxError> {
        if kinds.is_empty() {
            return Err(IdxError::InvalidInput(
                "name at least one kind of corporate action".to_string(),
            ));
        }
        let provider = corporate_action_provider();
        let subject = resolved.unwrap_or("corporate actions");
        let mut merged = Fetched {
            data: Vec::new(),
            source: stockbit::SOURCE,
            fetched_at: Utc::now(),
            stale: false,
        };
        let mut seen = Vec::with_capacity(kinds.len());
        for &kind in kinds {
            if seen.contains(&kind) {
                continue;
            }
            seen.push(kind);
            let key = format!("{}:{}", kind.as_str(), resolved.unwrap_or("market"));
            let part = cached_fetch(
                &self.idx.cache,
                CacheSpec {
                    bucket: BUCKET,
                    key: &key,
                    ttl_secs: TTL_SECS,
                    subject,
                    source: stockbit::SOURCE,
                },
                policy,
                || provider.corporate_actions(kind, resolved),
                Ok,
            )?;
            merged.fetched_at = merged.fetched_at.min(part.fetched_at);
            merged.stale |= part.stale;
            merged.data.extend(part.data);
        }
        Ok(merged)
    }
}
