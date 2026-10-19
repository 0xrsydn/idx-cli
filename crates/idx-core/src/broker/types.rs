//! Broker summary types: which IDX brokers bought and sold a stock.

use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// Whose trades a broker summary counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "snake_case")]
pub enum BrokerInvestor {
    /// Every trade.
    #[default]
    All,
    /// Trades by foreign investors only.
    Foreign,
    /// Trades by domestic investors only.
    Domestic,
}

impl BrokerInvestor {
    pub const ALL: [Self; 3] = [Self::All, Self::Foreign, Self::Domestic];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Foreign => "foreign",
            Self::Domestic => "domestic",
        }
    }
}

impl FromStr for BrokerInvestor {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "foreign" => Ok(Self::Foreign),
            "domestic" => Ok(Self::Domestic),
            _ => Err(IdxError::InvalidInput(format!(
                "invalid investor '{value}'; use all, foreign or domestic"
            ))),
        }
    }
}

/// Which market board's trades a broker summary counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "snake_case")]
pub enum BrokerBoard {
    /// Every board.
    #[default]
    All,
    /// Regular market (RG), the continuous order book.
    Regular,
    /// Cash market (TN).
    Cash,
    /// Negotiated market (NG): pre-arranged block trades.
    Negotiated,
}

impl BrokerBoard {
    pub const ALL: [Self; 4] = [Self::All, Self::Regular, Self::Cash, Self::Negotiated];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Regular => "regular",
            Self::Cash => "cash",
            Self::Negotiated => "negotiated",
        }
    }
}

impl FromStr for BrokerBoard {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "regular" => Ok(Self::Regular),
            "cash" => Ok(Self::Cash),
            "negotiated" => Ok(Self::Negotiated),
            _ => Err(IdxError::InvalidInput(format!(
                "invalid board '{value}'; use all, regular, cash or negotiated"
            ))),
        }
    }
}

/// The source's classification of a securities house. This is the broker's
/// own status, not the nationality of its clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerKind {
    Local,
    Foreign,
    /// State-owned securities house (BUMN).
    StateOwned,
}

impl BrokerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Foreign => "foreign",
            Self::StateOwned => "state_owned",
        }
    }
}

/// Which sessions a frontend asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerRange {
    /// The latest session with trades.
    Latest,
    /// `from..=to`, WIB dates.
    Dates { from: NaiveDate, to: NaiveDate },
}

impl BrokerRange {
    /// From a frontend's `date`, or `from` with an optional `to` (default
    /// today), as `YYYY-MM-DD`; none of them means the latest session.
    pub fn from_args(
        date: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        today: NaiveDate,
    ) -> Result<Self, IdxError> {
        let parse = |name: &str, value: &str| {
            NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
                IdxError::InvalidInput(format!("invalid {name} '{value}'; use YYYY-MM-DD"))
            })
        };
        match (date, from, to) {
            (None, None, None) => Ok(Self::Latest),
            (Some(date), None, None) => {
                let day = parse("date", date)?;
                Ok(Self::Dates { from: day, to: day })
            }
            (None, Some(from), to) => Ok(Self::Dates {
                from: parse("from", from)?,
                to: to.map(|to| parse("to", to)).transpose()?.unwrap_or(today),
            }),
            (None, None, Some(_)) => Err(IdxError::InvalidInput(
                "to needs from (or use date for one session)".to_string(),
            )),
            (Some(_), _, _) => Err(IdxError::InvalidInput(
                "date cannot be combined with from or to".to_string(),
            )),
        }
    }
}

/// The trading days a broker summary covers, both ends included (WIB dates).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
    #[serde(default)]
    pub investor: BrokerInvestor,
    #[serde(default)]
    pub board: BrokerBoard,
}

/// One broker on one side of a summary: gross buys for a buyer, gross sells
/// for a seller. A broker can appear on both sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerFlow {
    /// Position on its side as the source ranks it, 1 = most lots.
    pub rank: u32,
    /// IDX member code, e.g. `CC`.
    pub code: String,
    /// Member name from the bundled IDX member list; `None` for a code that
    /// is not on it.
    pub name: Option<String>,
    /// The source's classification; `None` when it gives none.
    pub kind: Option<BrokerKind>,
    /// Lots bought or sold (one lot is 100 shares).
    pub lots: Option<i64>,
    /// Value bought or sold, whole IDR.
    pub value: Option<i64>,
    /// Average price, whole IDR.
    pub average_price: Option<i64>,
}

/// The top buyers and sellers of one stock over a date range, as published.
/// Nothing is netted or estimated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerSummary {
    /// Resolved ticker, e.g. `BBCA.JK`.
    pub symbol: String,
    /// First trading date covered, as the source applied it.
    pub from: NaiveDate,
    /// Last trading date covered, as the source applied it.
    pub to: NaiveDate,
    pub investor: BrokerInvestor,
    pub board: BrokerBoard,
    /// Top buyers by lots, most first (up to 10).
    pub buyers: Vec<BrokerFlow>,
    /// Top sellers by lots, most first (up to 10).
    pub sellers: Vec<BrokerFlow>,
    /// Value traded over the range on the selected board, whole IDR. `None`
    /// with one investor side selected: the source's footer then covers only
    /// that side's buying, not the trades the lists show.
    pub total_value: Option<i64>,
    /// Lots traded over the range on the selected board; `None` as for
    /// `total_value`.
    pub total_lots: Option<i64>,
    /// Average traded price, whole IDR; `None` when nothing traded, or as
    /// for `total_value`.
    pub average_price: Option<i64>,
    /// Value the selected investors bought over the range on the selected
    /// board, whole IDR: `total_value` for all investors, and for one side
    /// the source's footer, which then covers that side's buying.
    pub buy_value: Option<i64>,
    /// Value the selected investors sold, whole IDR: `total_value` for all
    /// investors; for one side on all boards, its buying less its net
    /// (foreign net, or its negative for domestic investors). `None` for one
    /// side on one board, where the source gives no net to derive it from.
    pub sell_value: Option<i64>,
    /// Foreign buy value minus foreign sell value over the range, whole IDR,
    /// across all boards. The source does not apply the board filter to it,
    /// so it is `None` for a summary of one board.
    pub foreign_net_value: Option<i64>,
    /// True when the source abbreviated at least one figure (`1.6 M` lots,
    /// `841.2 B` IDR), so those figures carry only one decimal of precision.
    pub rounded: bool,
}

impl BrokerSummary {
    /// No trades in the range (weekend, holiday, suspension).
    pub fn is_empty(&self) -> bool {
        self.buyers.is_empty() && self.sellers.is_empty()
    }
}
