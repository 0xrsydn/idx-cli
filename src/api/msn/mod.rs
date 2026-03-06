mod client;
mod parse;
mod symbols;

use crate::api::MarketDataProvider;
use crate::api::types::{Fundamentals, Interval, Ohlc, Period, Quote};
use crate::error::IdxError;

use client::MsnClient;
use parse::{parse_fundamentals, parse_quote};

pub(crate) use parse::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

const HISTORY_UNSUPPORTED_REASON: &str = "MSN provider does not currently support history or technical analysis because MSN charts do not consistently expose real OHLCV data";

pub struct MsnProvider {
    client: MsnClient,
    verbose: bool,
}

impl MsnProvider {
    pub fn new(verbose: bool) -> Self {
        Self {
            client: MsnClient::new(),
            verbose,
        }
    }
}

impl MarketDataProvider for MsnProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let quotes = self.client.fetch_quotes(symbol)?;
        parse_quote(symbol, &quotes)
    }

    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        let ratios = self.client.fetch_key_ratios(symbol)?;
        let quote = self
            .client
            .fetch_quotes(symbol)
            .map_err(|e| {
                if self.verbose {
                    eprintln!("warning: quote fetch for fundamentals failed: {e}");
                }
                e
            })
            .ok()
            .and_then(|quotes| quotes.into_iter().next());
        parse_fundamentals(&ratios, quote.as_ref())
    }

    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        Err(IdxError::Unsupported(
            HISTORY_UNSUPPORTED_REASON.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::MsnProvider;
    use crate::api::MarketDataProvider;
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
