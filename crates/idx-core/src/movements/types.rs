use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// What a holder's row records, as Stockbit labels it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementAction {
    Buy,
    Sell,
    /// Shares moved between accounts or parties, not a market trade.
    Transfer,
    /// A crossing: a negotiated trade between two parties.
    Cross,
    RightsIssue,
    PrivatePlacement,
    TenderOffer,
    BonusStock,
    StockDividend,
    CashDividend,
    /// A label Stockbit added after this release.
    Other,
}

impl MovementAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Transfer => "transfer",
            Self::Cross => "cross",
            Self::RightsIssue => "rights_issue",
            Self::PrivatePlacement => "private_placement",
            Self::TenderOffer => "tender_offer",
            Self::BonusStock => "bonus_stock",
            Self::StockDividend => "stock_dividend",
            Self::CashDividend => "cash_dividend",
            Self::Other => "other",
        }
    }

    /// Whether `filter` (Stockbit's `action_type`) selects this action.
    pub fn matches(self, filter: ActionFilter) -> bool {
        match filter {
            ActionFilter::Buy => self == Self::Buy,
            ActionFilter::Sell => self == Self::Sell,
            ActionFilter::Transfer => self == Self::Transfer,
            ActionFilter::Cross => self == Self::Cross,
            ActionFilter::CorporateAction => matches!(
                self,
                Self::RightsIssue
                    | Self::PrivatePlacement
                    | Self::TenderOffer
                    | Self::BonusStock
                    | Self::StockDividend
                    | Self::CashDividend
            ),
        }
    }
}

/// The action filters Stockbit's feed takes (`action_type`). Corporate
/// action covers rights issues, private placements and the like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "kebab-case")]
pub enum ActionFilter {
    Buy,
    Sell,
    Transfer,
    Cross,
    #[cfg_attr(feature = "clap", value(name = "corporate-action"))]
    CorporateAction,
}

impl ActionFilter {
    pub const ALL: [Self; 5] = [
        Self::Buy,
        Self::Sell,
        Self::Transfer,
        Self::Cross,
        Self::CorporateAction,
    ];

    /// The CLI's name (`--action`), also used in MCP inputs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Transfer => "transfer",
            Self::Cross => "cross",
            Self::CorporateAction => "corporate-action",
        }
    }
}

impl std::str::FromStr for ActionFilter {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let wanted = value.trim().to_ascii_lowercase().replace('_', "-");
        Self::ALL
            .into_iter()
            .find(|filter| filter.as_str() == wanted)
            .ok_or_else(|| {
                IdxError::InvalidInput(format!(
                    "unknown action '{value}'; use one of: {}",
                    Self::ALL.map(Self::as_str).join(", ")
                ))
            })
    }
}

/// Who reported the change: IDX insider disclosures (directors,
/// commissioners and other insiders, any size), or KSEI's daily changes of
/// holders at or above 5%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum MovementSource {
    Idx,
    Ksei,
}

impl MovementSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idx => "idx",
            Self::Ksei => "ksei",
        }
    }
}

impl std::str::FromStr for MovementSource {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "idx" => Ok(Self::Idx),
            "ksei" => Ok(Self::Ksei),
            _ => Err(IdxError::InvalidInput(format!(
                "unknown source '{value}'; use idx or ksei"
            ))),
        }
    }
}

/// One reported change in a holder's stake. Share counts are exact;
/// percentages are of the company's shares, in basis points as Stockbit
/// rounds them, so a small insider's stake can read 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareholderMovement {
    pub symbol: String,
    /// The day of the change (the trade or the register date).
    pub date: NaiveDate,
    pub holder: String,
    /// Stockbit's id for the holder; `--holder` takes it.
    pub holder_id: String,
    pub action: MovementAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MovementSource>,
    /// `Local` or `Foreign`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    /// Insider roles at the company: `director`, `commissioner`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    pub shares_before: i64,
    pub shares_after: i64,
    /// Signed: negative for a sale.
    pub shares_change: i64,
    pub percentage_before_bps: i64,
    pub percentage_after_bps: i64,
    pub percentage_change_bps: i64,
    /// Whole IDR per share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<i64>,
    /// The price is Stockbit's estimate (KSEI rows report no price).
    pub price_estimated: bool,
    /// Whole IDR: shares changed times price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<i64>,
    /// The stated purpose, free text (`Investasi`, `Divestasi`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// One page of Stockbit's feed (50 rows), newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MovementPage {
    pub movements: Vec<ShareholderMovement>,
    /// Whether a further page exists.
    pub more: bool,
}

/// Most pages one query reads (500 rows).
pub const MAX_PAGES: u32 = 10;

/// Longest `from`..`to` Stockbit answers; a day more is `HTTP 400:
/// INVALID_PARAMETER` (checked 2026-11-13). One bound alone has no limit.
pub const MAX_RANGE_DAYS: i64 = 365;

/// What to list. `symbol` is resolved (`BBCA.JK`) once it reaches a
/// provider; `None` is the whole market.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MovementQuery {
    pub symbol: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub holder_id: Option<String>,
    pub action: Option<ActionFilter>,
    pub source: Option<MovementSource>,
    /// Pages to read, 1 to `MAX_PAGES`.
    pub pages: u32,
}

impl MovementQuery {
    /// Validates user input: dates as YYYY-MM-DD with `from` not after
    /// `to` and at most `MAX_RANGE_DAYS` before it, a numeric holder id, and 1 to `MAX_PAGES` pages.
    pub fn from_args(
        symbol: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        holder_id: Option<&str>,
        pages: u32,
    ) -> Result<Self, IdxError> {
        let parse = |name: &str, value: &str| {
            NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
                IdxError::InvalidInput(format!("invalid {name} '{value}'; use YYYY-MM-DD"))
            })
        };
        let from = from.map(|v| parse("from", v)).transpose()?;
        let to = to.map(|v| parse("to", v)).transpose()?;
        if let (Some(from), Some(to)) = (from, to)
            && to < from
        {
            return Err(IdxError::InvalidInput(format!(
                "to ({to}) is before from ({from})"
            )));
        }
        if let (Some(from), Some(to)) = (from, to)
            && (to - from).num_days() > MAX_RANGE_DAYS
        {
            return Err(IdxError::InvalidInput(format!(
                "from {from} to {to} is more than {MAX_RANGE_DAYS} days, the longest range \
                 Stockbit answers; narrow it, or give only one of the two"
            )));
        }
        let holder_id = holder_id.map(str::trim).filter(|id| !id.is_empty());
        if let Some(id) = holder_id
            && !id.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(IdxError::InvalidInput(format!(
                "holder '{id}' is not a holder id; ids are digits, shown in the HOLDER ID \
                 column and as holder_id in JSON"
            )));
        }
        if !(1..=MAX_PAGES).contains(&pages) {
            return Err(IdxError::InvalidInput(format!(
                "pages must be 1 to {MAX_PAGES}"
            )));
        }
        Ok(Self {
            symbol: symbol
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            from,
            to,
            holder_id: holder_id.map(str::to_string),
            action: None,
            source: None,
            pages,
        })
    }

    /// Whether `movement` passes the query's filters (what Stockbit applies
    /// on its side; the mock provider uses it).
    pub fn admits(&self, movement: &ShareholderMovement) -> bool {
        self.from.is_none_or(|from| movement.date >= from)
            && self.to.is_none_or(|to| movement.date <= to)
            && self
                .holder_id
                .as_ref()
                .is_none_or(|id| &movement.holder_id == id)
            && self
                .action
                .is_none_or(|filter| movement.action.matches(filter))
            && self
                .source
                .is_none_or(|source| movement.source == Some(source))
    }
}

/// The rows a query read, newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MovementList {
    pub movements: Vec<ShareholderMovement>,
    /// More rows exist past the pages read (raise `--pages`, or narrow the
    /// dates).
    pub more: bool,
    /// Why reading stopped before the pages asked for: a later page failed,
    /// and `movements` holds the rows read before it. Absent when every page
    /// asked for was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_validates_dates_holder_and_pages() {
        let query =
            MovementQuery::from_args(Some(" bbca "), Some("2026-10-01"), None, Some("14074"), 2)
                .unwrap();
        assert_eq!(query.symbol.as_deref(), Some("bbca"));
        assert_eq!(query.from, NaiveDate::from_ymd_opt(2026, 10, 1));
        assert_eq!(query.holder_id.as_deref(), Some("14074"));

        for (from, to, holder, pages) in [
            (Some("01-10-2026"), None, None, 1),
            (Some("2026-10-02"), Some("2026-10-01"), None, 1),
            (Some("2025-11-12"), Some("2026-11-13"), None, 1),
            (None, None, Some("RUDI"), 1),
            (None, None, Some("1&page=9"), 1),
            (None, None, None, 0),
            (None, None, None, MAX_PAGES + 1),
        ] {
            assert!(
                MovementQuery::from_args(None, from, to, holder, pages).is_err(),
                "{from:?} {to:?} {holder:?} {pages}"
            );
        }
    }

    /// Stockbit answers a year between the two dates and refuses a day more.
    #[test]
    fn query_takes_a_year_between_its_dates() {
        let year = MovementQuery::from_args(None, Some("2025-11-13"), Some("2026-11-13"), None, 1);
        assert!(year.is_ok());
        let err = MovementQuery::from_args(None, Some("2025-11-12"), Some("2026-11-13"), None, 1)
            .unwrap_err();
        assert!(err.to_string().contains("more than 365 days"), "{err}");
        // One bound alone is not a range.
        assert!(MovementQuery::from_args(None, Some("2010-01-01"), None, None, 1).is_ok());
    }

    #[test]
    fn action_filter_names_round_trip() {
        for filter in ActionFilter::ALL {
            assert_eq!(filter.as_str().parse::<ActionFilter>().unwrap(), filter);
        }
        assert_eq!(
            "corporate_action".parse::<ActionFilter>().unwrap(),
            ActionFilter::CorporateAction
        );
        assert!("dividend".parse::<ActionFilter>().is_err());
        assert!(MovementAction::RightsIssue.matches(ActionFilter::CorporateAction));
        assert!(!MovementAction::Transfer.matches(ActionFilter::CorporateAction));
    }
}
