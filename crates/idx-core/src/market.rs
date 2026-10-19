//! Market data: quotes, price history, and the screener.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc, Weekday};

use crate::api::types::{Interval, Ohlc, Period, Quote};
use crate::api::{history_provider, history_source, idx_offset};
use crate::client::{CachePolicy, CacheSpec, Fetched, Idx, cached_fetch};
use crate::config::{HistoryProviderKind, ProviderKind};
use crate::error::IdxError;

pub struct Market<'a> {
    pub(crate) idx: &'a Idx,
}

/// Which provider serves history, and whether that differs from the selected
/// market provider because `auto` fell back (MSN history is price-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySource {
    pub provider: ProviderKind,
    pub mode: HistoryProviderKind,
    pub is_fallback: bool,
}

impl Market<'_> {
    /// Quotes in input order; stops at the first symbol that fails.
    pub fn quotes<S: AsRef<str>>(
        &self,
        symbols: &[S],
        policy: CachePolicy,
    ) -> Result<Vec<Fetched<Quote>>, IdxError> {
        let idx = self.idx;
        let bucket = idx.bucket("quote");
        let session = latest_session(Utc::now());
        let mut quotes = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            let resolved = idx.resolve(symbol.as_ref())?;
            let mut quote = cached_fetch(
                &idx.cache,
                CacheSpec {
                    bucket: &bucket,
                    key: &resolved,
                    ttl_secs: idx.config.quote_ttl,
                    subject: &resolved,
                    source: idx.provider.kind().as_str(),
                },
                policy,
                || idx.provider.market().quote(&resolved),
                Ok,
            )?;
            // The session rule is IDX's; other exchanges keep other hours.
            if resolved.ends_with(".JK") {
                mark_behind_session(&mut quote.data, session);
            }
            quotes.push(quote);
        }
        Ok(quotes)
    }

    /// Resolves the history source for an optional `--history-provider`
    /// override, without fetching.
    pub fn history_source(&self, mode: Option<HistoryProviderKind>) -> HistorySource {
        let mode = mode.unwrap_or(self.idx.config.history_provider);
        let selected = self.idx.provider.kind();
        let provider = history_source(selected, mode);
        HistorySource {
            provider,
            mode,
            is_fallback: mode == HistoryProviderKind::Auto && provider != selected,
        }
    }

    pub fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
        source: HistorySource,
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<Ohlc>>, IdxError> {
        let idx = self.idx;
        let (provider_kind, provider) =
            history_provider(idx.provider.kind(), source.mode, idx.config.verbose)?;
        let bucket = format!("{}-history", provider_kind.as_str());
        let resolved = idx.resolve(symbol)?;
        let key = format!("{resolved}-{}-{}", period.as_str(), interval.as_str());
        cached_fetch(
            &idx.cache,
            CacheSpec {
                bucket: &bucket,
                key: &key,
                ttl_secs: idx.config.quote_ttl,
                subject: &resolved,
                source: provider_kind.as_str(),
            },
            policy,
            || provider.history(&resolved, period, interval),
            Ok,
        )
    }

    /// Screener results, ranked locally and truncated to `query.limit`.
    pub fn screen(
        &self,
        query: &ScreenQuery,
        policy: CachePolicy,
    ) -> Result<Fetched<Vec<Quote>>, IdxError> {
        let idx = self.idx;
        let ScreenQuery {
            filter,
            region,
            limit,
        } = *query;
        // Filters re-sorted client-side need the whole MSN list: MSN orders
        // by 1-year return, so truncating first and then sorting by day
        // change made `--limit 3` return different stocks than `--limit 25`.
        let fetch_limit = (!filter.needs_full_list()).then_some(limit);
        let bucket = idx.bucket("screen");
        let key = match fetch_limit {
            Some(limit) => format!("{filter}:{region}:{limit}"),
            None => format!("{filter}:{region}:all"),
        };
        let screener = idx.provider.screener_provider("screen")?;
        let mut fetched = cached_fetch(
            &idx.cache,
            CacheSpec {
                bucket: &bucket,
                key: &key,
                ttl_secs: idx.config.quote_ttl,
                subject: "screen",
                source: idx.provider.kind().as_str(),
            },
            policy,
            || screener.screener(filter.msn_key(), region.msn_key(), fetch_limit),
            Ok,
        )?;
        sort_screener_quotes(&mut fetched.data, filter);
        fetched.data.truncate(limit);
        if region == ScreenRegion::Id {
            let session = latest_session(Utc::now());
            for quote in &mut fetched.data {
                mark_behind_session(quote, session);
            }
        }
        Ok(fetched)
    }
}

/// When IDX's post-trading session ends, in WIB; MSN stamps a day's closing
/// quote with this time.
const SESSION_END: NaiveTime = NaiveTime::from_hms_opt(16, 15, 0).expect("valid time");

/// The latest weekday whose IDX session has ended by `now`. Exchange holidays
/// are not known offline, so on a weekday holiday this is the holiday itself.
pub(crate) fn latest_session(now: DateTime<Utc>) -> NaiveDate {
    let now = now.with_timezone(&idx_offset());
    let mut day = now.date_naive();
    if now.time() < SESSION_END {
        day -= Duration::days(1);
    }
    while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
        day -= Duration::days(1);
    }
    day
}

fn mark_behind_session(quote: &mut Quote, session: NaiveDate) {
    quote.behind_session = quote
        .as_of
        .is_some_and(|as_of| as_of.with_timezone(&idx_offset()).date_naive() < session);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenQuery {
    pub filter: ScreenFilter,
    pub region: ScreenRegion,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenFilter {
    TopPerformers,
    WorstPerformers,
    HighDividend,
    LowPe,
    Week52High,
    Week52Low,
    HighVolume,
    LargeCap,
}

impl ScreenFilter {
    pub const ALL: [Self; 8] = [
        Self::TopPerformers,
        Self::WorstPerformers,
        Self::HighDividend,
        Self::LowPe,
        Self::Week52High,
        Self::Week52Low,
        Self::HighVolume,
        Self::LargeCap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TopPerformers => "top-performers",
            Self::WorstPerformers => "worst-performers",
            Self::HighDividend => "high-dividend",
            Self::LowPe => "low-pe",
            Self::Week52High => "52w-high",
            Self::Week52Low => "52w-low",
            Self::HighVolume => "high-volume",
            Self::LargeCap => "large-cap",
        }
    }

    fn msn_key(self) -> &'static str {
        match self {
            Self::TopPerformers | Self::HighVolume | Self::LargeCap => "st_list_topperfs",
            Self::WorstPerformers => "st_list_poorperfs",
            Self::HighDividend => "st_list_highdividend",
            Self::LowPe => "st_list_lowpe",
            Self::Week52High => "st_list_52wkhi",
            Self::Week52Low => "st_list_52wklow",
        }
    }

    /// Filters ranked locally need every candidate before `limit` applies.
    fn needs_full_list(self) -> bool {
        matches!(
            self,
            Self::HighVolume | Self::LargeCap | Self::TopPerformers | Self::WorstPerformers
        )
    }
}

impl fmt::Display for ScreenFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScreenFilter {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, IdxError> {
        Self::ALL
            .into_iter()
            .find(|filter| filter.as_str() == value)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|f| f.as_str()).collect();
                IdxError::InvalidInput(format!(
                    "invalid screener filter '{value}' (expected one of: {})",
                    names.join(", ")
                ))
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenRegion {
    Id,
    Us,
    Sg,
    Hk,
    Jp,
}

impl ScreenRegion {
    pub const ALL: [Self; 5] = [Self::Id, Self::Us, Self::Sg, Self::Hk, Self::Jp];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Us => "us",
            Self::Sg => "sg",
            Self::Hk => "hk",
            Self::Jp => "jp",
        }
    }

    fn msn_key(self) -> &'static str {
        match self {
            Self::Id => "st_reg_id",
            Self::Us => "st_reg_us",
            Self::Sg => "st_reg_sg",
            Self::Hk => "st_reg_hk",
            Self::Jp => "st_reg_jp",
        }
    }
}

impl fmt::Display for ScreenRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScreenRegion {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, IdxError> {
        Self::ALL
            .into_iter()
            .find(|region| region.as_str() == value)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|r| r.as_str()).collect();
                IdxError::InvalidInput(format!(
                    "invalid screener region '{value}' (expected one of: {})",
                    names.join(", ")
                ))
            })
    }
}

fn sort_screener_quotes(quotes: &mut [Quote], filter: ScreenFilter) {
    match filter {
        ScreenFilter::TopPerformers => {
            quotes.sort_by(|a, b| {
                let pa = a.change_pct.unwrap_or(f64::MIN);
                let pb = b.change_pct.unwrap_or(f64::MIN);
                pb.partial_cmp(&pa).unwrap_or(Ordering::Equal)
            });
        }
        ScreenFilter::WorstPerformers => {
            quotes.sort_by(|a, b| {
                let pa = a.change_pct.unwrap_or(f64::MAX);
                let pb = b.change_pct.unwrap_or(f64::MAX);
                pa.partial_cmp(&pb).unwrap_or(Ordering::Equal)
            });
        }
        ScreenFilter::Week52High => {
            quotes.sort_by(|a, b| {
                let pa = a.week52_position.unwrap_or(f64::MIN);
                let pb = b.week52_position.unwrap_or(f64::MIN);
                pb.partial_cmp(&pa).unwrap_or(Ordering::Equal)
            });
        }
        ScreenFilter::Week52Low => {
            quotes.sort_by(|a, b| {
                let pa = a.week52_position.unwrap_or(f64::MAX);
                let pb = b.week52_position.unwrap_or(f64::MAX);
                pa.partial_cmp(&pb).unwrap_or(Ordering::Equal)
            });
        }
        ScreenFilter::HighVolume => {
            quotes.sort_by_key(|quote| std::cmp::Reverse(quote.volume));
        }
        ScreenFilter::LargeCap => {
            quotes.sort_by_key(|quote| std::cmp::Reverse(quote.market_cap.unwrap_or(0)));
        }
        ScreenFilter::HighDividend | ScreenFilter::LowPe => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ScreenFilter, ScreenRegion, latest_session, mark_behind_session, sort_screener_quotes,
    };
    use crate::api::types::Quote;
    use crate::error::IdxError;

    fn sample_quote(symbol: &str, change_pct: Option<f64>) -> Quote {
        Quote {
            symbol: symbol.into(),
            price: 100,
            change: None,
            change_pct,
            volume: None,
            market_cap: None,
            week52_high: None,
            week52_low: None,
            week52_position: None,
            range_signal: None,
            prev_close: None,
            avg_volume: None,
            as_of: None,
            behind_session: false,
        }
    }

    fn wib(raw: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(&format!("{raw}+07:00"))
            .unwrap()
            .to_utc()
    }

    fn day(raw: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn latest_session_is_the_last_weekday_past_the_post_trading_close() {
        // 2026-10-16 is a Friday.
        for (now, session) in [
            ("2026-10-16T16:53:00", "2026-10-16"),
            ("2026-10-16T16:15:00", "2026-10-16"),
            ("2026-10-16T16:14:59", "2026-10-15"),
            ("2026-10-16T00:30:00", "2026-10-15"),
            ("2026-10-17T12:00:00", "2026-10-16"),
            ("2026-10-18T23:59:59", "2026-10-16"),
            ("2026-10-19T10:00:00", "2026-10-16"),
            ("2026-10-19T16:15:00", "2026-10-19"),
        ] {
            assert_eq!(latest_session(wib(now)), day(session), "now {now}");
        }
    }

    #[test]
    fn quote_from_before_the_latest_session_is_behind_it() {
        // Issue #50: at 16:53 WIB on the 16th, MSN still served the 15th.
        let session = latest_session(wib("2026-10-16T16:53:00"));
        for (as_of, behind) in [
            (Some("2026-10-15T16:15:00+07:00"), true),
            (Some("2026-10-16T16:15:00+07:00"), false),
            (Some("2026-10-16T09:15:00Z"), false),
            (Some("2026-10-15T17:30:00Z"), false),
            (Some("2025-02-17T16:14:58+07:00"), true),
            (None, false),
        ] {
            let mut quote = sample_quote("BBCA.JK", None);
            quote.as_of = as_of.map(|t| chrono::DateTime::parse_from_rfc3339(t).unwrap());
            mark_behind_session(&mut quote, session);
            assert_eq!(quote.behind_session, behind, "as_of {as_of:?}");
        }
    }

    #[test]
    fn sort_screener_top_performers_descending() {
        let mut quotes = vec![
            sample_quote("A", Some(1.0)),
            sample_quote("B", Some(5.0)),
            sample_quote("C", Some(3.0)),
        ];
        sort_screener_quotes(&mut quotes, ScreenFilter::TopPerformers);
        let pcts: Vec<f64> = quotes.iter().filter_map(|q| q.change_pct).collect();
        assert_eq!(pcts, vec![5.0, 3.0, 1.0]);
    }

    #[test]
    fn sort_screener_worst_performers_ascending() {
        let mut quotes = vec![
            sample_quote("A", Some(1.0)),
            sample_quote("B", Some(-5.0)),
            sample_quote("C", Some(-2.0)),
        ];
        sort_screener_quotes(&mut quotes, ScreenFilter::WorstPerformers);
        let pcts: Vec<f64> = quotes.iter().filter_map(|q| q.change_pct).collect();
        assert_eq!(pcts, vec![-5.0, -2.0, 1.0]);
    }

    #[test]
    fn sort_screener_puts_unknown_change_last_both_ways() {
        let mut quotes = vec![
            sample_quote("U", None),
            sample_quote("A", Some(1.0)),
            sample_quote("B", Some(-5.0)),
        ];
        sort_screener_quotes(&mut quotes, ScreenFilter::WorstPerformers);
        assert_eq!(quotes.last().unwrap().symbol, "U");
        sort_screener_quotes(&mut quotes, ScreenFilter::TopPerformers);
        assert_eq!(quotes.last().unwrap().symbol, "U");
    }

    #[test]
    fn invalid_screener_filter_returns_error() {
        let err = "bogus"
            .parse::<ScreenFilter>()
            .expect_err("invalid filter should fail");
        assert!(matches!(err, IdxError::InvalidInput(_)));
        assert!(err.to_string().contains("invalid screener filter"));
    }

    #[test]
    fn invalid_screener_region_returns_error() {
        let err = "eu"
            .parse::<ScreenRegion>()
            .expect_err("invalid region should fail");
        assert!(matches!(err, IdxError::InvalidInput(_)));
        assert!(err.to_string().contains("invalid screener region"));
    }

    #[test]
    fn screen_names_round_trip() {
        for filter in ScreenFilter::ALL {
            assert_eq!(filter.as_str().parse::<ScreenFilter>().unwrap(), filter);
        }
        for region in ScreenRegion::ALL {
            assert_eq!(region.as_str().parse::<ScreenRegion>().unwrap(), region);
        }
    }
}
