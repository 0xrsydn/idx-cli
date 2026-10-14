mod client;
mod map;
mod parse;
mod raw_types;

use crate::api::types::{Bar, Fundamentals, Interval, Period, Quote};
use crate::api::{FundamentalsProvider, HistoryProvider, QuoteProvider};
use crate::error::IdxError;

use client::YahooClient;
use map::{parse_fundamentals, parse_quote};
use parse::parse_history_with_verbose;

pub(crate) use parse::{
    parse_fundamentals_from_str, parse_history_from_str_with_verbose, parse_quote_from_str,
};

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

impl QuoteProvider for YahooProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let chart = self
            .client
            .fetch_chart(symbol, &Period::OneDay, &Interval::Day)?;
        parse_quote(symbol, &chart)
    }
}

impl FundamentalsProvider for YahooProvider {
    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError> {
        let quote_summary = self.client.fetch_quote_summary(symbol)?;
        parse_fundamentals(symbol, &quote_summary)
    }
}

impl HistoryProvider for YahooProvider {
    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        let chart = self.client.fetch_chart(symbol, period, interval)?;
        parse_history_with_verbose(symbol, &chart, self.verbose)
    }
}
