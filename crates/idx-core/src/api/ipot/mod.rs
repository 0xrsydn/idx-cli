//! Indo Premier Sekuritas (IPOT) broker summary: which brokers bought and
//! sold a stock. No login or key.
//!
//! `GET https://www.indopremier.com/module/saham/include/data-brokersummary.php`
//! `?code=BBCA&start=MM/DD/YYYY&end=MM/DD/YYYY&fd=all|F|D&board=all|RG|TN|NG`
//! answers an HTML fragment with the top 10 buyers and the top 10 sellers,
//! ranked by lots, and totals for the range. Figures of a million and more
//! are abbreviated to one decimal (`1.6 M`). A range without trades gives
//! blank rows and zero totals; an unknown code gives `Invalid stock code!`
//! with HTTP 200. The endpoint's behavior was first mapped by fzn0x/idx
//! (MIT).

mod client;
pub mod parse;

use client::IpotClient;
pub use parse::{BrokerSummaryRaw, IpotPage, parse_broker_summary, parse_figure};

use super::BrokerSummaryProvider;
use crate::broker::types::{BrokerBoard, BrokerInvestor, BrokerQuery};
use crate::error::IdxError;

const ENDPOINT: &str = "https://www.indopremier.com/module/saham/include/data-brokersummary.php";

/// The request URL for `code` (a bare IDX code such as `BBCA`).
pub fn broker_summary_url(code: &str, query: &BrokerQuery) -> String {
    let investor = match query.investor {
        BrokerInvestor::All => "all",
        BrokerInvestor::Foreign => "F",
        BrokerInvestor::Domestic => "D",
    };
    let board = match query.board {
        BrokerBoard::All => "all",
        BrokerBoard::Regular => "RG",
        BrokerBoard::Cash => "TN",
        BrokerBoard::Negotiated => "NG",
    };
    format!(
        "{ENDPOINT}?code={code}&start={}&end={}&fd={investor}&board={board}",
        query.from.format("%m/%d/%Y"),
        query.to.format("%m/%d/%Y"),
    )
}

/// A parsed page as a summary; an unknown code is `SymbolNotFound`.
pub(crate) fn summary_from_html(code: &str, html: &str) -> Result<BrokerSummaryRaw, IdxError> {
    match parse_broker_summary(html)? {
        IpotPage::Invalid => Err(IdxError::SymbolNotFound(code.to_string())),
        IpotPage::Summary(raw) => Ok(raw),
    }
}

pub struct IpotProvider {
    client: IpotClient,
}

impl IpotProvider {
    pub fn new() -> Self {
        Self {
            client: IpotClient::new(),
        }
    }
}

impl Default for IpotProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BrokerSummaryProvider for IpotProvider {
    fn broker_summary(
        &self,
        code: &str,
        query: &BrokerQuery,
    ) -> Result<BrokerSummaryRaw, IdxError> {
        let html = self.client.fetch_html(&broker_summary_url(code, query))?;
        summary_from_html(code, &html)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn test_broker_summary_url_maps_filters_to_source_codes() {
        let query = BrokerQuery {
            from: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 10, 16).unwrap(),
            investor: BrokerInvestor::Foreign,
            board: BrokerBoard::Regular,
        };
        assert_eq!(
            broker_summary_url("BBCA", &query),
            "https://www.indopremier.com/module/saham/include/data-brokersummary.php?code=BBCA&start=10/12/2026&end=10/16/2026&fd=F&board=RG"
        );
    }
}
