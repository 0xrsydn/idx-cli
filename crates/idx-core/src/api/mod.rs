#[cfg(feature = "testing")]
pub mod mock;
pub mod msn;
pub mod types;
pub mod yahoo;

use chrono::FixedOffset;

use crate::config::{HistoryProviderKind, ProviderKind};
use crate::error::IdxError;
use types::{
    Bar, CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    NewsItem, Period, Quote, SentimentData,
};

/// IDX trades in Western Indonesia Time (WIB, UTC+7, no DST). Providers stamp
/// candles in UTC, so day boundaries must be taken in this zone.
pub(crate) const IDX_UTC_OFFSET_SECS: i32 = 7 * 3600;

pub(crate) fn idx_offset() -> FixedOffset {
    FixedOffset::east_opt(IDX_UTC_OFFSET_SECS).expect("valid WIB offset")
}

/// Parse a provider's RFC 3339 trade time and express it in WIB.
pub(crate) fn parse_as_of(raw: Option<&str>) -> Option<chrono::DateTime<FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(raw?)
        .ok()
        .map(|t| t.with_timezone(&idx_offset()))
}

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
    /// `None` requests the complete candidate list for client-side ranking.
    fn screener(
        &self,
        filter: &str,
        region: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Quote>, IdxError>;
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
    build_selected_provider(provider, verbose, mock_enabled())
}

fn mock_enabled() -> bool {
    #[cfg(feature = "testing")]
    return mock::enabled();
    #[cfg(not(feature = "testing"))]
    false
}

#[cfg(feature = "testing")]
fn mock_market(provider: ProviderKind) -> Option<Box<dyn MarketDataProvider>> {
    Some(Box::new(mock::MockProvider::from_fixtures(provider)))
}

#[cfg(not(feature = "testing"))]
fn mock_market(_provider: ProviderKind) -> Option<Box<dyn MarketDataProvider>> {
    None
}

#[cfg(feature = "testing")]
fn mock_history(provider: ProviderKind, verbose: bool) -> Option<Box<dyn HistoryProvider>> {
    Some(Box::new(
        mock::MockProvider::from_fixtures_with_history_verbose(provider, verbose),
    ))
}

#[cfg(not(feature = "testing"))]
fn mock_history(_provider: ProviderKind, _verbose: bool) -> Option<Box<dyn HistoryProvider>> {
    None
}

fn build_selected_provider(
    provider: ProviderKind,
    verbose: bool,
    use_mock: bool,
) -> SelectedProvider {
    let mock = if use_mock {
        mock_market(provider)
    } else {
        None
    };
    match provider {
        ProviderKind::Yahoo => SelectedProvider {
            kind: ProviderKind::Yahoo,
            market: mock.unwrap_or_else(|| Box::new(yahoo::YahooProvider::new(verbose))),
            profile: None,
            financials: None,
            earnings: None,
            sentiment: None,
            insights: None,
            news: None,
            screener: None,
        },
        ProviderKind::Msn => SelectedProvider {
            kind: ProviderKind::Msn,
            market: mock.unwrap_or_else(|| Box::new(msn::MsnProvider::new(verbose))),
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

    if mock_enabled()
        && let Some(mock) = mock_history(resolved, verbose)
    {
        return Ok((resolved, mock));
    }

    match resolved {
        ProviderKind::Yahoo => Ok((resolved, Box::new(yahoo::YahooProvider::new(verbose)))),
        ProviderKind::Msn => Ok((resolved, Box::new(msn::MsnProvider::new(verbose)))),
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

    #[cfg(feature = "testing")]
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

    #[cfg(feature = "testing")]
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
