#[allow(dead_code, clippy::enum_variant_names)]
pub mod bing;
mod client;
mod map;
mod parse;
mod raw_types;
mod symbols;

use crate::api::types::{
    Bar, CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    NewsItem, Period, Quote, SentimentData,
};
use crate::api::{
    EarningsProvider, FinancialsProvider, FundamentalsProvider, HistoryProvider, InsightsProvider,
    NewsProvider, ProfileProvider, QuoteProvider, ScreenerProvider, SentimentProvider,
};
use crate::error::IdxError;

use client::MsnClient;
use map::{
    parse_earnings, parse_financial_statements, parse_fundamentals, parse_history, parse_insights,
    parse_news, parse_profile, parse_quote, parse_screener_results, parse_sentiment,
};

pub(crate) use parse::{parse_fundamentals_from_str, parse_history_from_str, parse_quote_from_str};

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
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        let raw = self.client.fetch_chart(symbol, period, interval)?;
        parse_history(symbol, &raw)
    }
}

impl ProfileProvider for MsnProvider {
    fn profile(&self, symbol: &str) -> Result<CompanyProfile, IdxError> {
        let raw = self.client.fetch_equities(symbol)?;
        parse_profile(symbol, &raw)
    }
}

impl EarningsProvider for MsnProvider {
    fn earnings(&self, symbol: &str) -> Result<EarningsReport, IdxError> {
        let raw = self.client.fetch_earnings(symbol)?;
        parse_earnings(symbol, &raw)
    }
}

impl FinancialsProvider for MsnProvider {
    fn financials(&self, symbol: &str) -> Result<FinancialStatements, IdxError> {
        let raw = self.client.fetch_financial_statements(symbol)?;
        parse_financial_statements(symbol, &raw)
    }
}

impl SentimentProvider for MsnProvider {
    fn sentiment(&self, symbol: &str) -> Result<SentimentData, IdxError> {
        let raw = self.client.fetch_sentiment(symbol)?;
        parse_sentiment(symbol, &raw)
    }
}

impl InsightsProvider for MsnProvider {
    fn insights(&self, symbol: &str) -> Result<InsightData, IdxError> {
        let raw = self.client.fetch_insights(symbol)?;
        parse_insights(symbol, &raw)
    }
}

impl NewsProvider for MsnProvider {
    fn news(&self, symbol: &str, limit: usize) -> Result<Vec<NewsItem>, IdxError> {
        let raw = self.client.fetch_news(symbol, limit)?;
        parse_news(symbol, &raw)
    }
}

impl ScreenerProvider for MsnProvider {
    fn screener(&self, filter: &str, region: &str, limit: usize) -> Result<Vec<Quote>, IdxError> {
        let raw = self.client.fetch_screener(filter, region, limit)?;
        parse_screener_results(&raw)
    }
}
