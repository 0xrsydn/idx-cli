use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// The kinds of corporate action Stockbit lists, one request each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Cash dividend.
    Dividend,
    #[cfg_attr(feature = "clap", value(name = "stock-dividend"))]
    StockDividend,
    /// Bonus shares.
    Bonus,
    /// Rights issue (HMETD).
    Rights,
    Warrant,
    /// Stock split.
    Split,
    #[cfg_attr(feature = "clap", value(name = "reverse-split"))]
    ReverseSplit,
    /// Shareholders' meeting (RUPS).
    Meeting,
    /// Public expose.
    #[cfg_attr(feature = "clap", value(name = "expose"))]
    PublicExpose,
    Ipo,
    #[cfg_attr(feature = "clap", value(name = "tender"))]
    TenderOffer,
}

impl ActionKind {
    pub const ALL: [Self; 11] = [
        Self::Dividend,
        Self::StockDividend,
        Self::Bonus,
        Self::Rights,
        Self::Warrant,
        Self::Split,
        Self::ReverseSplit,
        Self::Meeting,
        Self::PublicExpose,
        Self::Ipo,
        Self::TenderOffer,
    ];

    /// What one stock's history covers by default: everything but its IPO.
    pub const STOCK_DEFAULT: [Self; 10] = [
        Self::Dividend,
        Self::StockDividend,
        Self::Bonus,
        Self::Rights,
        Self::Warrant,
        Self::Split,
        Self::ReverseSplit,
        Self::Meeting,
        Self::PublicExpose,
        Self::TenderOffer,
    ];

    /// The CLI's name (`--kind`), also used in MCP inputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dividend => "dividend",
            Self::StockDividend => "stock-dividend",
            Self::Bonus => "bonus",
            Self::Rights => "rights",
            Self::Warrant => "warrant",
            Self::Split => "split",
            Self::ReverseSplit => "reverse-split",
            Self::Meeting => "meeting",
            Self::PublicExpose => "expose",
            Self::Ipo => "ipo",
            Self::TenderOffer => "tender",
        }
    }

    /// Whether the action entitles holders on a cum date (and so moves the
    /// price on the ex date).
    pub fn is_entitlement(self) -> bool {
        matches!(
            self,
            Self::Dividend
                | Self::StockDividend
                | Self::Bonus
                | Self::Rights
                | Self::Split
                | Self::ReverseSplit
        )
    }
}

impl std::str::FromStr for ActionKind {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|k| k.as_str()).collect();
                IdxError::InvalidInput(format!(
                    "invalid kind '{value}' (expected one of: {})",
                    names.join(", ")
                ))
            })
    }
}

/// `old : new` as Stockbit states it: a 1:5 split turns 1 share into 5; a
/// 655:104 rights issue gives 104 rights per 655 shares held; a 6:4 bonus
/// gives 4 bonus shares per 6 held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ratio {
    pub old: u64,
    pub new: u64,
}

/// Inclusive dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// One corporate action of one stock. Which fields are present depends on
/// `kind`; absent ones are left out of JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorporateAction {
    /// `BBCA.JK`.
    pub symbol: String,
    pub kind: ActionKind,
    /// Where the action sits on a calendar: the cum date for entitlements
    /// (the last day to buy and be entitled), else the meeting or expose
    /// day, the listing day, a warrant's first trading day, or a tender
    /// offer's first day.
    pub date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cum_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ex_date: Option<NaiveDate>,
    /// Recording date; for a meeting, the date that decides who may attend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pay_date: Option<NaiveDate>,
    /// Cash dividend per share, IDR as Stockbit gives it (up to 2 decimals,
    /// e.g. 3.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cash_per_share: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<Ratio>,
    /// Stockbit's price adjustment factor: 5 for a 1:5 split, 1.6667 for a
    /// 6:4 bonus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<f64>,
    /// Whole IDR: the rights exercise price, a warrant's exercise price, the
    /// tender offer price or the final IPO price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<i64>,
    /// When rights or warrants trade, a tender offer runs, or an IPO is
    /// offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<DateRange>,
    /// When warrants can be exercised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exercise: Option<DateRange>,
    /// Shares offered (IPO) or sought (tender offer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shares: Option<u64>,
    /// Share of the company a tender offer seeks, basis points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage_bps: Option<i64>,
    /// Meeting or expose time, as Stockbit writes it (WIB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// The issuer's name, when Stockbit gives it (IPO, tender offer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl CorporateAction {
    pub fn new(symbol: String, kind: ActionKind, date: NaiveDate) -> Self {
        Self {
            symbol,
            kind,
            date,
            cum_date: None,
            ex_date: None,
            record_date: None,
            pay_date: None,
            cash_per_share: None,
            ratio: None,
            factor: None,
            price: None,
            period: None,
            exercise: None,
            shares: None,
            percentage_bps: None,
            time: None,
            venue: None,
            name: None,
        }
    }
}

/// Default calendar span, days from today.
pub const CALENDAR_DEFAULT_DAYS: i64 = 30;
/// Longest calendar range, days.
pub const CALENDAR_MAX_DAYS: i64 = 366;

/// The dates a market-wide calendar covers, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl CalendarRange {
    /// `from` defaults to today, `to` to 30 days after `from`.
    pub fn from_args(
        from: Option<&str>,
        to: Option<&str>,
        today: NaiveDate,
    ) -> Result<Self, IdxError> {
        let parse = |name: &str, value: &str| {
            NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
                IdxError::InvalidInput(format!("invalid {name} '{value}'; use YYYY-MM-DD"))
            })
        };
        let from = from.map(|v| parse("from", v)).transpose()?.unwrap_or(today);
        let to = to
            .map(|v| parse("to", v))
            .transpose()?
            .unwrap_or(from + Duration::days(CALENDAR_DEFAULT_DAYS));
        if to < from {
            return Err(IdxError::InvalidInput(format!(
                "to ({to}) is before from ({from})"
            )));
        }
        if (to - from).num_days() > CALENDAR_MAX_DAYS {
            return Err(IdxError::InvalidInput(format!(
                "a calendar covers at most {CALENDAR_MAX_DAYS} days"
            )));
        }
        Ok(Self { from, to })
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        (self.from..=self.to).contains(&date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn test_calendar_range_defaults_and_bounds() {
        let today = day("2026-11-03");
        let range = CalendarRange::from_args(None, None, today).unwrap();
        assert_eq!((range.from, range.to), (today, day("2026-12-03")));
        let range = CalendarRange::from_args(Some("2026-11-10"), None, today).unwrap();
        assert_eq!(range.to, day("2026-12-10"));
        for (from, to) in [
            (Some("2026-11-10"), Some("2026-11-09")),
            (Some("2026-01-01"), Some("2027-06-01")),
            (Some("11/10/2026"), None),
        ] {
            assert!(matches!(
                CalendarRange::from_args(from, to, today),
                Err(IdxError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn test_action_kind_names_round_trip() {
        for kind in ActionKind::ALL {
            assert_eq!(kind.as_str().parse::<ActionKind>().unwrap(), kind);
        }
        assert!("rups".parse::<ActionKind>().is_err());
    }
}
