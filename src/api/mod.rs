pub mod msn;
pub mod types;
pub mod yahoo;

use crate::config::{HistoryProviderKind, ProviderKind};
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

#[allow(dead_code)]
pub trait ScreenerProvider {
    fn screener(&self, filter: &str, region: &str, limit: usize) -> Result<Vec<Quote>, IdxError>;
}

pub struct SelectedProvider {
    kind: ProviderKind,
    market: Box<dyn MarketDataProvider>,
    profile: Option<Box<dyn ProfileProvider>>,
    financials: Option<Box<dyn FinancialsProvider>>,
    earnings: Option<Box<dyn EarningsProvider>>,
    sentiment: Option<Box<dyn SentimentProvider>>,
    insights: Option<Box<dyn InsightsProvider>>,
    news: Option<Box<dyn NewsProvider>>,
    screener: Option<Box<dyn ScreenerProvider>>,
}

impl SelectedProvider {
    pub fn kind(&self) -> ProviderKind {
        self.kind
    }

    pub fn market(&self) -> &dyn MarketDataProvider {
        self.market.as_ref()
    }

    pub fn profile_provider(&self, subject: &str) -> Result<&dyn ProfileProvider, IdxError> {
        self.profile
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn financials_provider(&self, subject: &str) -> Result<&dyn FinancialsProvider, IdxError> {
        self.financials
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn earnings_provider(&self, subject: &str) -> Result<&dyn EarningsProvider, IdxError> {
        self.earnings
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn sentiment_provider(&self, subject: &str) -> Result<&dyn SentimentProvider, IdxError> {
        self.sentiment
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn insights_provider(&self, subject: &str) -> Result<&dyn InsightsProvider, IdxError> {
        self.insights
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn news_provider(&self, subject: &str) -> Result<&dyn NewsProvider, IdxError> {
        self.news
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }

    pub fn screener_provider(&self, subject: &str) -> Result<&dyn ScreenerProvider, IdxError> {
        self.screener
            .as_deref()
            .ok_or_else(|| msn_capability_error(subject))
    }
}

pub fn resolve_symbol(symbol: &str, exchange: &str) -> Result<String, IdxError> {
    let trimmed = symbol.trim().to_uppercase();
    if trimmed.is_empty() {
        return Err(IdxError::InvalidInput(
            "ticker symbol cannot be empty".into(),
        ));
    }
    if let Some((base, suffix)) = trimmed.rsplit_once('.')
        && !base.is_empty()
        && !suffix.is_empty()
    {
        return Ok(trimmed);
    }
    Ok(format!("{trimmed}.{}", exchange.trim().to_uppercase()))
}

pub fn default_provider(provider: ProviderKind, verbose: bool) -> SelectedProvider {
    build_selected_provider(
        provider,
        verbose,
        std::env::var("IDX_USE_MOCK_PROVIDER").is_ok(),
    )
}

fn build_selected_provider(
    provider: ProviderKind,
    verbose: bool,
    use_mock: bool,
) -> SelectedProvider {
    match (provider, use_mock) {
        (ProviderKind::Yahoo, true) => SelectedProvider {
            kind: ProviderKind::Yahoo,
            market: Box::new(MockProvider::from_fixtures(ProviderKind::Yahoo)),
            profile: None,
            financials: None,
            earnings: None,
            sentiment: None,
            insights: None,
            news: None,
            screener: None,
        },
        (ProviderKind::Yahoo, false) => SelectedProvider {
            kind: ProviderKind::Yahoo,
            market: Box::new(yahoo::YahooProvider::new(verbose)),
            profile: None,
            financials: None,
            earnings: None,
            sentiment: None,
            insights: None,
            news: None,
            screener: None,
        },
        (ProviderKind::Msn, true) => SelectedProvider {
            kind: ProviderKind::Msn,
            market: Box::new(MockProvider::from_fixtures(ProviderKind::Msn)),
            profile: Some(Box::new(msn::MsnProvider::new(verbose))),
            financials: Some(Box::new(msn::MsnProvider::new(verbose))),
            earnings: Some(Box::new(msn::MsnProvider::new(verbose))),
            sentiment: Some(Box::new(msn::MsnProvider::new(verbose))),
            insights: Some(Box::new(msn::MsnProvider::new(verbose))),
            news: Some(Box::new(msn::MsnProvider::new(verbose))),
            screener: Some(Box::new(msn::MsnProvider::new(verbose))),
        },
        (ProviderKind::Msn, false) => SelectedProvider {
            kind: ProviderKind::Msn,
            market: Box::new(msn::MsnProvider::new(verbose)),
            profile: Some(Box::new(msn::MsnProvider::new(verbose))),
            financials: Some(Box::new(msn::MsnProvider::new(verbose))),
            earnings: Some(Box::new(msn::MsnProvider::new(verbose))),
            sentiment: Some(Box::new(msn::MsnProvider::new(verbose))),
            insights: Some(Box::new(msn::MsnProvider::new(verbose))),
            news: Some(Box::new(msn::MsnProvider::new(verbose))),
            screener: Some(Box::new(msn::MsnProvider::new(verbose))),
        },
    }
}

fn msn_capability_error(subject: &str) -> IdxError {
    IdxError::Unsupported(format!("{subject}: command requires --provider msn"))
}

/// Resolves a history provider based on the selected market data provider and
/// history provider strategy.
///
/// `history_mode=auto` keeps using Yahoo for IDX history because Yahoo provides
/// full OHLCV candles. Explicit `msn` opts into MSN's price-only chart feed.
pub fn history_provider(
    provider: ProviderKind,
    history_mode: HistoryProviderKind,
    verbose: bool,
) -> Result<(ProviderKind, Box<dyn HistoryProvider>), IdxError> {
    let resolved = match history_mode {
        HistoryProviderKind::Yahoo => ProviderKind::Yahoo,
        HistoryProviderKind::Msn => ProviderKind::Msn,
        HistoryProviderKind::Auto => match provider {
            ProviderKind::Yahoo => ProviderKind::Yahoo,
            ProviderKind::Msn => ProviderKind::Yahoo,
        },
    };

    if std::env::var("IDX_USE_MOCK_PROVIDER").is_ok() {
        return Ok((
            resolved,
            Box::new(MockProvider::from_fixtures_with_history_verbose(
                resolved, verbose,
            )),
        ));
    }

    match resolved {
        ProviderKind::Yahoo => Ok((resolved, Box::new(yahoo::YahooProvider::new(verbose)))),
        ProviderKind::Msn => Ok((resolved, Box::new(msn::MsnProvider::new(verbose)))),
    }
}

pub struct MockProvider {
    quote: Result<Quote, IdxError>,
    fundamentals: Result<Fundamentals, IdxError>,
    history: Result<Vec<Bar>, IdxError>,
}

impl MockProvider {
    pub fn from_fixtures(provider: ProviderKind) -> Self {
        Self::from_fixtures_with_history_verbose(provider, false)
    }

    pub fn from_fixtures_with_history_verbose(
        provider: ProviderKind,
        history_verbose: bool,
    ) -> Self {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Self::with_error(IdxError::ProviderUnavailable);
        }

        match provider {
            ProviderKind::Yahoo => Self::from_yahoo_fixtures(history_verbose),
            ProviderKind::Msn => Self::from_msn_fixtures(),
        }
    }

    fn from_yahoo_fixtures(history_verbose: bool) -> Self {
        let quote_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json")
            .unwrap_or_else(|_| "{}".to_string());
        let history_path = std::env::var("IDX_MOCK_YAHOO_HISTORY_FIXTURE")
            .unwrap_or_else(|_| "tests/fixtures/chart_bbca_3mo.json".to_string());
        let history_raw =
            std::fs::read_to_string(&history_path).unwrap_or_else(|_| "{}".to_string());
        let fundamentals_raw = std::fs::read_to_string("tests/fixtures/quotesummary_bbca.json")
            .unwrap_or_else(|_| "{}".to_string());

        let quote = yahoo::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = yahoo::parse_fundamentals_from_str("BBCA.JK", &fundamentals_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history =
            yahoo::parse_history_from_str_with_verbose("BBCA.JK", &history_raw, history_verbose)
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
        let fundamentals_path = std::env::var("IDX_MOCK_MSN_KEYRATIOS_FIXTURE")
            .unwrap_or_else(|_| "tests/fixtures/msn_keyratios_bbca.json".to_string());
        let fundamentals_raw =
            std::fs::read_to_string(&fundamentals_path).unwrap_or_else(|_| "[]".to_string());

        let quote = msn::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let fundamentals = msn::parse_fundamentals_from_str(&fundamentals_raw, Some(&quote_raw))
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history_raw = std::fs::read_to_string("tests/fixtures/msn_chart_bbca_3m.json")
            .unwrap_or_else(|_| "[]".to_string());
        let history = msn::parse_history_from_str("BBCA.JK", &history_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));

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
    use super::{ProviderKind, build_selected_provider, resolve_symbol};
    use crate::error::IdxError;

    #[test]
    fn resolves_symbol_variants() {
        assert_eq!(resolve_symbol("bbca", "JK").unwrap(), "BBCA.JK");
        assert_eq!(resolve_symbol("BBCA.JK", "JK").unwrap(), "BBCA.JK");
        assert_eq!(resolve_symbol("TLKM.us", "JK").unwrap(), "TLKM.US");
        assert_eq!(resolve_symbol("abcd.ef.gh", "JK").unwrap(), "ABCD.EF.GH");
        assert_eq!(resolve_symbol(" bbri ", "jk").unwrap(), "BBRI.JK");
        // Empty ticker should return error
        assert!(resolve_symbol("", "JK").is_err());
        // Whitespace-only ticker should also return error
        assert!(resolve_symbol("  ", "JK").is_err());
        // Valid ticker returns Ok
        assert_eq!(resolve_symbol("BBCA", "JK").unwrap(), "BBCA.JK");
    }

    #[test]
    fn selected_provider_exposes_expected_capabilities_for_msn() {
        let provider = build_selected_provider(ProviderKind::Msn, false, false);

        assert!(provider.profile_provider("BBCA.JK").is_ok());
        assert!(provider.financials_provider("BBCA.JK").is_ok());
        assert!(provider.earnings_provider("BBCA.JK").is_ok());
        assert!(provider.sentiment_provider("BBCA.JK").is_ok());
        assert!(provider.insights_provider("BBCA.JK").is_ok());
        assert!(provider.news_provider("BBCA.JK").is_ok());
        assert!(provider.screener_provider("screen").is_ok());
    }

    #[test]
    fn selected_provider_exposes_expected_capabilities_for_mock_msn() {
        let provider = build_selected_provider(ProviderKind::Msn, false, true);

        assert!(provider.profile_provider("BBCA.JK").is_ok());
        assert!(provider.financials_provider("BBCA.JK").is_ok());
        assert!(provider.earnings_provider("BBCA.JK").is_ok());
        assert!(provider.sentiment_provider("BBCA.JK").is_ok());
        assert!(provider.insights_provider("BBCA.JK").is_ok());
        assert!(provider.news_provider("BBCA.JK").is_ok());
        assert!(provider.screener_provider("screen").is_ok());
    }

    #[test]
    fn selected_provider_rejects_msn_only_capabilities_for_yahoo() {
        let provider = build_selected_provider(ProviderKind::Yahoo, false, false);
        let err = match provider.profile_provider("BBCA.JK") {
            Ok(_) => panic!("yahoo should not expose msn-only profile"),
            Err(err) => err,
        };

        assert!(matches!(err, IdxError::Unsupported(_)));
        assert_eq!(
            err.to_string(),
            "unsupported: BBCA.JK: command requires --provider msn"
        );
    }

    #[test]
    fn selected_provider_rejects_msn_only_capabilities_for_mock_yahoo() {
        let provider = build_selected_provider(ProviderKind::Yahoo, false, true);
        let err = match provider.screener_provider("screen") {
            Ok(_) => panic!("mock yahoo should not expose msn-only screener"),
            Err(err) => err,
        };

        assert!(matches!(err, IdxError::Unsupported(_)));
        assert_eq!(
            err.to_string(),
            "unsupported: screen: command requires --provider msn"
        );
    }
}
