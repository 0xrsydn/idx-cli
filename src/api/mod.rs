pub mod msn;
pub mod types;
pub mod yahoo;

use crate::config::ProviderKind;
use crate::error::IdxError;
use types::{
    Bar, CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    NewsItem, Period, Quote, SentimentData,
};

pub trait QuoteProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError>;
}

pub trait HistoryProvider {
    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError>;
}

pub trait FundamentalsProvider {
    fn fundamentals(&self, symbol: &str) -> Result<Fundamentals, IdxError>;
}

/// Core provider trait — quote + fundamentals only.
/// History is a separate capability (`HistoryProvider`) not all providers support
/// (e.g. MSN Finance/Charts returns 404 for IDX/XIDX stocks).
pub trait MarketDataProvider: QuoteProvider + FundamentalsProvider {}
impl<T> MarketDataProvider for T where T: QuoteProvider + FundamentalsProvider {}

#[allow(dead_code)]
pub trait ProfileProvider {
    fn profile(&self, symbol: &str) -> Result<CompanyProfile, IdxError>;
}

#[allow(dead_code)]
pub trait EarningsProvider {
    fn earnings(&self, symbol: &str) -> Result<EarningsReport, IdxError>;
}

#[allow(dead_code)]
pub trait FinancialsProvider {
    fn financials(&self, symbol: &str) -> Result<FinancialStatements, IdxError>;
}

#[allow(dead_code)]
pub trait SentimentProvider {
    fn sentiment(&self, symbol: &str) -> Result<SentimentData, IdxError>;
}

#[allow(dead_code)]
pub trait InsightsProvider {
    fn insights(&self, symbol: &str) -> Result<InsightData, IdxError>;
}

#[allow(dead_code)]
pub trait NewsProvider {
    fn news(&self, symbol: &str, limit: usize) -> Result<Vec<NewsItem>, IdxError>;
}

pub fn resolve_symbol(symbol: &str, exchange: &str) -> String {
    let trimmed = symbol.trim().to_uppercase();
    if let Some((base, suffix)) = trimmed.rsplit_once('.')
        && !base.is_empty()
        && !suffix.is_empty()
    {
        return trimmed;
    }
    format!("{trimmed}.{}", exchange.trim().to_uppercase())
}

pub fn default_provider(provider: ProviderKind, verbose: bool) -> Box<dyn MarketDataProvider> {
    if std::env::var("IDX_USE_MOCK_PROVIDER").is_ok() {
        Box::new(MockProvider::from_fixtures(provider))
    } else {
        match provider {
            ProviderKind::Yahoo => Box::new(yahoo::YahooProvider::new(verbose)),
            ProviderKind::Msn => Box::new(msn::MsnProvider::new(verbose)),
        }
    }
}

/// Returns a history-capable provider, or `None` if the selected provider doesn't
/// support price history (e.g. MSN Finance/Charts returns 404 for IDX/XIDX stocks).
pub fn history_provider(provider: ProviderKind, verbose: bool) -> Option<Box<dyn HistoryProvider>> {
    if std::env::var("IDX_USE_MOCK_PROVIDER").is_ok() {
        return Some(Box::new(MockProvider::from_fixtures(provider)));
    }
    match provider {
        ProviderKind::Yahoo => Some(Box::new(yahoo::YahooProvider::new(verbose))),
        ProviderKind::Msn => None,
    }
}

pub struct MockProvider {
    quote: Result<Quote, IdxError>,
    fundamentals: Result<Fundamentals, IdxError>,
    history: Result<Vec<Bar>, IdxError>,
}

impl MockProvider {
    pub fn from_fixtures(provider: ProviderKind) -> Self {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Self::with_error(IdxError::ProviderUnavailable);
        }

        match provider {
            ProviderKind::Yahoo => Self::from_yahoo_fixtures(),
            ProviderKind::Msn => Self::from_msn_fixtures(),
        }
    }

    fn from_yahoo_fixtures() -> Self {
        let quote_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json")
            .unwrap_or_else(|_| "{}".to_string());
        let history_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_3mo.json")
            .unwrap_or_else(|_| "{}".to_string());
        let fundamentals_raw = std::fs::read_to_string("tests/fixtures/quotesummary_bbca.json")
            .unwrap_or_else(|_| "{}".to_string());

        let quote = yahoo::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = yahoo::parse_fundamentals_from_str("BBCA.JK", &fundamentals_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history = yahoo::parse_history_from_str("BBCA.JK", &history_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));

        Self {
            quote,
            fundamentals,
            history,
        }
    }

    fn from_msn_fixtures() -> Self {
        let quote_raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .unwrap_or_else(|_| "[]".to_string());
        let fundamentals_raw = std::fs::read_to_string("tests/fixtures/msn_keyratios_bbca.json")
            .unwrap_or_else(|_| "[]".to_string());

        let quote = msn::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = msn::parse_fundamentals_from_str(&fundamentals_raw, Some(&quote_raw))
            .map_err(|e| IdxError::ParseError(e.to_string()));
        // MSN Finance/Charts returns 404 for IDX (XIDX) — history not supported
        let history = Err(IdxError::Unsupported(
            "MSN does not provide price history for IDX stocks. Use --provider yahoo.".into(),
        ));

        Self {
            quote,
            fundamentals,
            history,
        }
    }

    pub fn with_error(err: IdxError) -> Self {
        Self {
            quote: Err(err.clone()),
            fundamentals: Err(err.clone()),
            history: Err(err),
        }
    }
}

impl QuoteProvider for MockProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let mut q = self.quote.clone()?;
        q.symbol = symbol.to_string();
        Ok(q)
    }
}

impl FundamentalsProvider for MockProvider {
    fn fundamentals(&self, _symbol: &str) -> Result<Fundamentals, IdxError> {
        self.fundamentals.clone()
    }
}

impl HistoryProvider for MockProvider {
    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Bar>, IdxError> {
        self.history.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_symbol;

    #[test]
    fn resolves_symbol_variants() {
        assert_eq!(resolve_symbol("bbca", "JK"), "BBCA.JK");
        assert_eq!(resolve_symbol("BBCA.JK", "JK"), "BBCA.JK");
        assert_eq!(resolve_symbol("TLKM.us", "JK"), "TLKM.US");
        assert_eq!(resolve_symbol("abcd.ef.gh", "JK"), "ABCD.EF.GH");
        assert_eq!(resolve_symbol(" bbri ", "jk"), "BBRI.JK");
        assert_eq!(resolve_symbol("", "JK"), ".JK");
    }
}
