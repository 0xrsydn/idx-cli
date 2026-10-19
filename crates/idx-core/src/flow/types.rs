//! Foreign flow types: what foreign investors bought and sold of a stock,
//! session by session (end of day), from IPOT or IDX's trading summary.

use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// Sessions shown when a frontend gives neither days nor dates.
pub const DEFAULT_SESSIONS: usize = 10;

/// Where a foreign flow comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    /// Indo Premier's broker summary: foreign buy, sell and net value across
    /// all boards, two plain requests per session. Figures of a million and
    /// more are rounded to one decimal.
    #[default]
    Ipot,
    /// IDX's own trading summary: exact foreign share counts, close and the
    /// non-regular share, regular board. Needs curl-impersonate and a
    /// network IDX's Cloudflare lets through.
    Idx,
}

impl FlowSource {
    pub const ALL: [Self; 2] = [Self::Ipot, Self::Idx];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ipot => "ipot",
            Self::Idx => "idx",
        }
    }
}

impl FromStr for FlowSource {
    type Err = IdxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ipot" => Ok(Self::Ipot),
            "idx" => Ok(Self::Idx),
            _ => Err(IdxError::InvalidInput(format!(
                "invalid source '{value}'; use ipot or idx"
            ))),
        }
    }
}

/// One stock's trading summary for one session, as IDX publishes it.
/// Prices are whole IDR, volumes are shares. Volume, value and the foreign
/// figures cover the regular board only (across a week of every stock, no
/// foreign figure exceeded regular volume, even where non-regular volume was
/// more than twice it); the non-regular figures cover the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockDay {
    /// Bare IDX code, e.g. `BBCA`.
    pub code: String,
    pub date: NaiveDate,
    pub previous: Option<i64>,
    pub high: Option<i64>,
    pub low: Option<i64>,
    pub close: Option<i64>,
    pub volume: Option<i64>,
    /// Value traded on the regular board, whole IDR.
    pub value: Option<i64>,
    pub frequency: Option<i64>,
    /// Shares bought by foreign investors.
    pub foreign_buy: Option<i64>,
    /// Shares sold by foreign investors.
    pub foreign_sell: Option<i64>,
    /// Shares traded outside the regular board (negotiated and cash).
    pub non_regular_volume: Option<i64>,
    /// Value traded outside the regular board, whole IDR.
    pub non_regular_value: Option<i64>,
}

/// Which sessions a foreign flow covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowRange {
    /// The latest `sessions` sessions with trades.
    Latest { sessions: usize },
    /// Every session in `from..=to` (WIB dates).
    Dates { from: NaiveDate, to: NaiveDate },
}

impl FlowRange {
    /// From a frontend's `days`, or `from` with an optional `to` (default
    /// today) as `YYYY-MM-DD`; neither means the latest
    /// [`DEFAULT_SESSIONS`].
    pub fn from_args(
        days: Option<usize>,
        from: Option<&str>,
        to: Option<&str>,
        today: NaiveDate,
    ) -> Result<Self, IdxError> {
        let parse = |name: &str, value: &str| {
            NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
                IdxError::InvalidInput(format!("invalid {name} '{value}'; use YYYY-MM-DD"))
            })
        };
        match (days, from, to) {
            (days, None, None) => Ok(Self::Latest {
                sessions: days.unwrap_or(DEFAULT_SESSIONS),
            }),
            (None, Some(from), to) => Ok(Self::Dates {
                from: parse("from", from)?,
                to: to.map(|to| parse("to", to)).transpose()?.unwrap_or(today),
            }),
            (None, None, Some(_)) => Err(IdxError::InvalidInput("to needs from".to_string())),
            (Some(_), _, _) => Err(IdxError::InvalidInput(
                "days cannot be combined with from or to".to_string(),
            )),
        }
    }
}

/// One session. Which figures are present depends on the source: IPOT gives
/// values across all boards, IDX gives exact shares on the regular board (and
/// values estimated from them) with the close and the non-regular share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignFlowDay {
    pub date: NaiveDate,
    /// Close, whole IDR (IDX only).
    pub close: Option<i64>,
    /// Close against the previous close, basis points (IDX only).
    pub change_bps: Option<i64>,
    /// Shares traded: the regular board (IDX), or all boards from lots
    /// (IPOT).
    pub volume: Option<i64>,
    /// Value traded, whole IDR: the regular board (IDX) or all boards (IPOT).
    pub value: Option<i64>,
    /// Shares foreign investors bought (IDX only).
    pub foreign_buy: Option<i64>,
    /// Shares foreign investors sold (IDX only).
    pub foreign_sell: Option<i64>,
    /// `foreign_buy - foreign_sell`, shares (IDX only).
    pub foreign_net: Option<i64>,
    /// Value foreign investors bought, whole IDR: as published (IPOT), or
    /// estimated at the session's average price (IDX).
    pub foreign_buy_value: Option<i64>,
    /// Value foreign investors sold, whole IDR: their buying less their net
    /// (IPOT), or estimated at the session's average price (IDX).
    pub foreign_sell_value: Option<i64>,
    /// Foreign buy value minus sell value, whole IDR: as published (IPOT), or
    /// net shares at the session's average price (IDX, an estimate).
    pub foreign_net_value: Option<i64>,
    /// Foreign buying and selling over twice the session's trading: foreign
    /// investors' share of it, basis points.
    pub foreign_share_bps: Option<i64>,
    /// Value traded outside the regular board (negotiated and cash), basis
    /// points of all value traded (IDX only).
    pub non_regular_bps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignFlow {
    /// Resolved ticker, e.g. `BBCA.JK`.
    pub symbol: String,
    pub source: FlowSource,
    /// First and last session covered.
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Sessions the stock traded, oldest first.
    pub days: Vec<ForeignFlowDay>,
    /// Net shares over the sessions (IDX only); `None` when a session lacks
    /// a figure.
    pub foreign_net: Option<i64>,
    /// Foreign buy value over the sessions, whole IDR.
    pub foreign_buy_value: Option<i64>,
    /// Foreign sell value over the sessions, whole IDR.
    pub foreign_sell_value: Option<i64>,
    /// Foreign net value over the sessions, whole IDR.
    pub foreign_net_value: Option<i64>,
    /// Foreign investors' share of trading over the sessions, basis points.
    pub foreign_share_bps: Option<i64>,
    /// The source rounded figures of a million and more to one decimal.
    pub rounded: bool,
}
