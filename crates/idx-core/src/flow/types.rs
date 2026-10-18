//! Foreign flow types: what foreign investors bought and sold of a stock,
//! session by session (IDX trading summary, end of day).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::IdxError;

/// Sessions shown when a frontend gives neither days nor dates.
pub const DEFAULT_SESSIONS: usize = 10;

/// One stock's trading summary for one session, as IDX publishes it.
/// Prices are whole IDR, volumes are shares.
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
    /// Value traded, whole IDR, all boards.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignFlowDay {
    pub date: NaiveDate,
    pub close: Option<i64>,
    /// Close against the previous close, basis points.
    pub change_bps: Option<i64>,
    /// Shares traded.
    pub volume: Option<i64>,
    /// Value traded, whole IDR.
    pub value: Option<i64>,
    /// Shares foreign investors bought.
    pub foreign_buy: Option<i64>,
    /// Shares foreign investors sold.
    pub foreign_sell: Option<i64>,
    /// `foreign_buy - foreign_sell`, shares.
    pub foreign_net: Option<i64>,
    /// `foreign_net` at the session's average price (value over volume),
    /// whole IDR: an estimate.
    pub foreign_net_value: Option<i64>,
    /// Value traded outside the regular board (negotiated and cash), basis
    /// points of the session's value.
    pub non_regular_bps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignFlow {
    /// Resolved ticker, e.g. `BBCA.JK`.
    pub symbol: String,
    /// First and last session covered.
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Sessions the stock traded, oldest first.
    pub days: Vec<ForeignFlowDay>,
    /// Net shares over the sessions; `None` when a session lacks a figure.
    pub foreign_net: Option<i64>,
    /// Sum of the sessions' estimated net values, whole IDR.
    pub foreign_net_value: Option<i64>,
    /// Foreign buys and sells over twice the volume: foreign investors'
    /// share of trading, basis points.
    pub foreign_share_bps: Option<i64>,
}
