mod client;
mod parse;
mod symbols;

use crate::api::MarketDataProvider;
use crate::api::types::{Fundamentals, Interval, Ohlc, Period, Quote};
use crate::error::IdxError;

use client::MsnClient;
use parse::{
    ResampleInterval, parse_fundamentals, parse_history_with_verbose, parse_quote, resample_history,
};

pub(crate) use parse::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

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

    fn chart_type_for_period(period: &Period) -> &'static str {
        match period {
            Period::OneDay => "1D1M",
            Period::FiveDays | Period::OneMonth => "1M",
            Period::ThreeMonths => "3M",
            Period::SixMonths | Period::OneYear => "1Y",
            Period::TwoYears => "3Y",
            Period::FiveYears => "5Y",
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
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        let charts = self
            .client
            .fetch_charts(symbol, Self::chart_type_for_period(period))?;
        let rows = parse_history_with_verbose(period, &charts, self.verbose)?;
        Ok(match interval {
            Interval::Day => rows,
            Interval::Week => resample_history(&rows, ResampleInterval::Week),
            Interval::Month => resample_history(&rows, ResampleInterval::Month),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MsnProvider;
    use crate::api::types::Period;

    #[test]
    fn maps_periods_to_supported_chart_types() {
        assert_eq!(MsnProvider::chart_type_for_period(&Period::OneDay), "1D1M");
        assert_eq!(MsnProvider::chart_type_for_period(&Period::SixMonths), "1Y");
        assert_eq!(MsnProvider::chart_type_for_period(&Period::TwoYears), "3Y");
        assert_eq!(MsnProvider::chart_type_for_period(&Period::FiveYears), "5Y");
    }
}
