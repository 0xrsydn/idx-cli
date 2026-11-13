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
        let short = starts_after(&fetched.data, range.from);
        if range.from < self.today() && !short.is_empty() {
            let kinds: Vec<String> = short
                .iter()
                .map(|(kind, first)| format!("{} from {first}", kind.as_str()))
                .collect();
            crate::runtime::warn(format!(
                "Stockbit's market-wide lists start after {}: {}; earlier events of those kinds \
                 are missing here (`stocks actions SYM` has one stock's full history)",
                range.from,
                kinds.join(", ")
            ));
        }
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

/// The kinds whose oldest row is dated after `from`, with that date.
/// Stockbit's market-wide list of a kind holds its newest rows only (three
/// months of meetings, a year of dividends on 2026-11-13), so a range that
/// starts before the oldest row is not covered from its start.
fn starts_after(actions: &[CorporateAction], from: NaiveDate) -> Vec<(ActionKind, NaiveDate)> {
    let mut oldest: Vec<(ActionKind, NaiveDate)> = Vec::new();
    for action in actions {
        match oldest.iter_mut().find(|(kind, _)| *kind == action.kind) {
            Some((_, date)) => *date = (*date).min(action.date),
            None => oldest.push((action.kind, action.date)),
        }
    }
    oldest.retain(|(_, date)| *date > from);
    oldest.sort();
    oldest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    /// A kind whose oldest row is after the range's first day is named with
    /// that row's date; one that reaches the first day is not.
    #[test]
    fn kinds_that_start_after_the_range_are_named() {
        let row = |kind, date| CorporateAction::new("BBCA.JK".to_string(), kind, date);
        let actions = [
            row(ActionKind::Meeting, day(9, 2)),
            row(ActionKind::Meeting, day(8, 3)),
            row(ActionKind::Dividend, day(1, 5)),
            row(ActionKind::Dividend, day(6, 1)),
        ];
        assert_eq!(
            starts_after(&actions, day(3, 1)),
            [(ActionKind::Meeting, day(8, 3))]
        );
        assert!(starts_after(&actions, day(8, 3)).is_empty());
        assert!(starts_after(&[], day(1, 1)).is_empty());
    }
}
