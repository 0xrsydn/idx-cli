mod client;
mod map;
mod parse;
mod raw_types;
mod symbols;

use crate::api::types::{Bar, Fundamentals, Interval, Period, Quote};
use crate::api::{FundamentalsProvider, HistoryProvider, QuoteProvider};
use crate::error::IdxError;

use client::MsnClient;
use map::{parse_fundamentals, parse_quote};

pub(crate) use parse::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

const HISTORY_UNSUPPORTED_REASON: &str = "MSN provider does not currently support history or technical analysis because MSN charts do not consistently expose real OHLCV data";

pub struct MsnProvider {
    client: MsnClient,
}

impl MsnProvider {
    pub fn new(_verbose: bool) -> Self {
        Self {
            client: MsnClient::new(),
        }
    }
}

impl QuoteProvider for MsnProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let quotes = self.client.fetch_quotes(symbol)?;
        parse_quote(symbol, &quotes)
    }
}

impl FundamentalsProvider for MsnProvider {
    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        let ratios = self.client.fetch_key_ratios(symbol)?;
        let quote = self.client.fetch_quotes(symbol)?;
        parse_fundamentals(&ratios, quote.first())
    }
}

impl HistoryProvider for MsnProvider {
    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        Err(IdxError::Unsupported(
            HISTORY_UNSUPPORTED_REASON.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::MsnProvider;
    use crate::api::HistoryProvider;
    use crate::api::types::{Interval, Period};
    use crate::error::IdxError;

    #[test]
    fn history_is_explicitly_unsupported() {
        let provider = MsnProvider::new(false);
        let err = provider
            .history("BBCA.JK", &Period::OneMonth, &Interval::Day)
            .expect_err("history should be unsupported");
        assert!(matches!(err, IdxError::Unsupported(_)));
        assert!(
            err.to_string()
                .contains("MSN provider does not currently support history or technical analysis")
        );
    }
}
