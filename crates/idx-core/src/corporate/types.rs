use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// The kinds of corporate action Stockbit lists, one request each. JSON
/// names them as `--kind` does (`as_str`), so an output kind can be passed
/// back in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    /// Cash dividend.
    Dividend,
    #[cfg_attr(
        feature = "clap",
        value(name = "stock-dividend", alias = "stock_dividend")
    )]
    #[serde(alias = "stock_dividend")]
    StockDividend,
    /// Bonus shares.
    Bonus,
    /// Rights issue (HMETD).
    Rights,
    Warrant,
    /// Stock split.
    Split,
    #[cfg_attr(
        feature = "clap",
        value(name = "reverse-split", alias = "reverse_split")
    )]
    #[serde(alias = "reverse_split")]
    ReverseSplit,
    /// Shareholders' meeting (RUPS).
    Meeting,
    /// Public expose.
    #[cfg_attr(
        feature = "clap",
        value(name = "expose", alias = "public-expose", alias = "public_expose")
    )]
    #[serde(rename = "expose", alias = "public_expose")]
    PublicExpose,
    Ipo,
    #[cfg_attr(
        feature = "clap",
        value(name = "tender", alias = "tender-offer", alias = "tender_offer")
    )]
    #[serde(rename = "tender", alias = "tender_offer")]
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
        let wanted = value.trim().to_ascii_lowercase().replace('_', "-");
        // The names JSON carried before they matched `--kind`, which an
        // agent may pass back from an earlier answer.
        let wanted = match wanted.as_str() {
            "public-expose" => "expose",
            "tender-offer" => "tender",
            other => other,
        };
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == wanted)
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
    /// offer's first day. An entitlement Stockbit gives no cum date for is
    /// placed on its ex date, and `cum_date` is then absent.
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
    /// Cash dividend per share as Stockbit gives it: IDR to the sen (e.g.
    /// 3.8), another `currency` to 6 decimals (e.g. USD 0.0012).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cash_per_share: Option<f64>,
    /// The dividend's currency (ISO code, e.g. `USD`) when it is not IDR;
    /// absent for IDR, which every recorded dividend was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
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
            currency: None,
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
    /// `from` defaults to today, `to` to 30 days after `from`; at most
    /// `CALENDAR_MAX_DAYS` days, counting both ends.
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
        let to = match to {
            Some(v) => parse("to", v)?,
            None => from
                .checked_add_signed(Duration::days(CALENDAR_DEFAULT_DAYS))
                .ok_or_else(|| IdxError::InvalidInput(format!("from ({from}) is out of range")))?,
        };
        if to < from {
            return Err(IdxError::InvalidInput(format!(
                "to ({to}) is before from ({from})"
            )));
        }
        if (to - from).num_days() >= CALENDAR_MAX_DAYS {
            return Err(IdxError::InvalidInput(format!(
                "a calendar covers at most {CALENDAR_MAX_DAYS} days"
            )));
        }
        Ok(Self { from, to })
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        (self.from..=self.to).contains(&date)
    }

    /// Whether `action` belongs on this calendar: dated in it, or an offer
    /// still open in it (rights trading, a tender offer, an IPO offer), so a
    /// calendar shows what can still be acted on. A warrant's windows run
    /// for years, so a warrant counts by its date alone.
    pub fn includes(&self, action: &CorporateAction) -> bool {
        let open_offer = matches!(
            action.kind,
            ActionKind::Rights | ActionKind::TenderOffer | ActionKind::Ipo
        ) && action
            .period
            .is_some_and(|period| period.start <= self.to && period.end >= self.from);
        self.contains(action.date) || open_offer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// A calendar shows what is dated in it and the offers still open in it;
    /// a warrant's years-long windows do not put it on every calendar.
    #[test]
    fn test_calendar_includes_offers_still_open() {
        let range = CalendarRange {
            from: day("2026-11-03"),
            to: day("2026-11-12"),
        };
        let with_period = |kind, date: &str, start: &str, end: &str| {
            let mut action = CorporateAction::new("AUTO.JK".into(), kind, day(date));
            action.period = Some(DateRange {
                start: day(start),
                end: day(end),
            });
            action
        };
        let tender = with_period(
            ActionKind::TenderOffer,
            "2026-10-13",
            "2026-10-13",
            "2026-11-11",
        );
        assert!(range.includes(&tender), "open until 11-11");
        let rights = with_period(ActionKind::Rights, "2026-10-20", "2026-10-28", "2026-11-03");
        assert!(range.includes(&rights), "trades on the first day");
        let closed = with_period(
            ActionKind::TenderOffer,
            "2026-09-01",
            "2026-09-01",
            "2026-11-02",
        );
        assert!(!range.includes(&closed), "closed the day before");
        let warrant = with_period(
            ActionKind::Warrant,
            "2025-01-10",
            "2025-01-10",
            "2028-01-10",
        );
        assert!(!range.includes(&warrant), "a warrant counts by its date");
        let dated = CorporateAction::new("BBCA.JK".into(), ActionKind::Dividend, day("2026-11-12"));
        assert!(range.includes(&dated));
    }

    #[test]
    fn test_calendar_range_defaults_and_bounds() {
        let today = day("2026-11-03");
        let range = CalendarRange::from_args(None, None, today).unwrap();
        assert_eq!((range.from, range.to), (today, day("2026-12-03")));
        let range = CalendarRange::from_args(Some("2026-11-10"), None, today).unwrap();
        assert_eq!(range.to, day("2026-12-10"));
        // 366 days counting both ends is the most.
        let range = CalendarRange::from_args(Some("2028-01-01"), Some("2028-12-31"), today);
        assert!(range.is_ok());
        for (from, to) in [
            (Some("2026-11-10"), Some("2026-11-09")),
            (Some("2026-01-01"), Some("2027-06-01")),
            (Some("2028-01-01"), Some("2029-01-01")),
            (Some("11/10/2026"), None),
        ] {
            assert!(matches!(
                CalendarRange::from_args(from, to, today),
                Err(IdxError::InvalidInput(_))
            ));
        }
    }

    /// The last date there is: 30 days on is an error, not a panic.
    #[test]
    fn test_calendar_range_at_the_end_of_time() {
        let last = NaiveDate::MAX.format("%Y-%m-%d").to_string();
        let err = CalendarRange::from_args(Some(&last), None, day("2026-11-03")).unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    #[test]
    fn test_action_kind_names_round_trip() {
        for kind in ActionKind::ALL {
            assert_eq!(kind.as_str().parse::<ActionKind>().unwrap(), kind);
        }
        assert!("rups".parse::<ActionKind>().is_err());
        assert_eq!(
            "stock_dividend".parse::<ActionKind>().unwrap(),
            ActionKind::StockDividend
        );
    }

    /// JSON's `kind` is the name `--kind` and MCP take.
    #[test]
    fn test_action_kind_json_is_the_input_name() {
        for kind in ActionKind::ALL {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, kind.as_str(), "{kind:?}");
            assert_eq!(serde_json::from_value::<ActionKind>(json).unwrap(), kind);
        }
        // Entries cached before the names changed still read.
        let old: ActionKind = serde_json::from_str("\"public_expose\"").unwrap();
        assert_eq!(old, ActionKind::PublicExpose);
        // And as input, so a kind copied from an older answer works.
        assert_eq!("public_expose".parse::<ActionKind>().unwrap(), old);
        assert_eq!(
            "tender_offer".parse::<ActionKind>().unwrap(),
            ActionKind::TenderOffer
        );
    }
}
