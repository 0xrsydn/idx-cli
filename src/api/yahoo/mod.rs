mod client;
mod parse;

use crate::api::MarketDataProvider;
use crate::api::types::{Fundamentals, Interval, Ohlc, Period, Quote};
use crate::error::IdxError;

use client::YahooClient;
use parse::{parse_fundamentals, parse_history_with_verbose, parse_quote};

pub(crate) use parse::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

pub struct YahooProvider {
    client: YahooClient,
    verbose: bool,
}

impl YahooProvider {
    pub fn new(verbose: bool) -> Self {
        Self {
            client: YahooClient::new(),
            verbose,
        }
    }
}

impl MarketDataProvider for YahooProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let chart = self
            .client
            .fetch_chart(symbol, &Period::OneDay, &Interval::Day)?;
        parse_quote(symbol, &chart)
    }

    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        let quote_summary = self.client.fetch_quote_summary(symbol)?;
        parse_fundamentals(symbol, &quote_summary)
    }

    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        let chart = self.client.fetch_chart(symbol, period, interval)?;
        parse_history_with_verbose(&chart, self.verbose)
    }
}
