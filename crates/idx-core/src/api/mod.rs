pub mod idx;
pub mod ipot;
#[cfg(feature = "testing")]
pub mod mock;
pub mod msn;
pub mod stockbit;
pub mod types;
pub mod yahoo;

use chrono::FixedOffset;

use crate::broker::types::BrokerQuery;
use crate::config::{HistoryProviderKind, ProviderKind};
use crate::error::IdxError;
use types::{
    Bar, CompanyProfile, EarningsReport, FinancialStatements, Fundamentals, InsightData, Interval,
    Intraday, NewsItem, OrderBook, Period, Quote, SentimentData,
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

/// Which brokers bought and sold a stock: a separate capability, served by
/// IPOT whichever market provider is selected.
pub trait BrokerSummaryProvider {
    /// `code` is the bare IDX code, e.g. `BBCA`.
    fn broker_summary(
        &self,
        code: &str,
        query: &BrokerQuery,
    ) -> Result<ipot::BrokerSummaryRaw, IdxError>;
}

/// Every stock's trading summary for one session (IDX's own site).
pub trait TradingSummaryProvider {
    /// Empty for a day without trading.
    fn stock_summary(
        &self,
        date: chrono::NaiveDate,
    ) -> Result<Vec<crate::flow::types::StockDay>, IdxError>;
}

/// Today's session of one IDX symbol: quote and price per minute. Served by
/// Stockbit's public page whichever market provider is selected.
pub trait IntradayProvider {
    /// `symbol` is resolved, e.g. `BBCA.JK`.
    fn intraday(&self, symbol: &str) -> Result<Intraday, IdxError>;
}

/// The order book of one IDX stock: needs the user's Stockbit session.
pub trait OrderBookProvider {
    /// `symbol` is resolved, e.g. `BBCA.JK`.
    fn orderbook(&self, symbol: &str) -> Result<OrderBook, IdxError>;
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
    // Tickers name cache files and provider URLs: letters, digits, and the
    // separators real symbols use (BBCA.JK, BRK-B, ^JKSE), nothing else.
    if trimmed.len() > 32
        || trimmed.contains("..")
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '^' | '='))
    {
        return Err(IdxError::InvalidInput(format!(
            "invalid ticker symbol '{}'",
            symbol.trim()
        )));
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
    match provider {
        ProviderKind::Stockbit => Some(Box::new(mock::MockStockbitProvider)),
        _ => Some(Box::new(mock::MockProvider::from_fixtures(provider))),
    }
}

#[cfg(not(feature = "testing"))]
fn mock_market(_provider: ProviderKind) -> Option<Box<dyn MarketDataProvider>> {
    None
}

#[cfg(feature = "testing")]
fn mock_history(provider: ProviderKind, verbose: bool) -> Option<Box<dyn HistoryProvider>> {
    match provider {
        ProviderKind::Stockbit => Some(Box::new(mock::MockStockbitProvider)),
        _ => Some(Box::new(
            mock::MockProvider::from_fixtures_with_history_verbose(provider, verbose),
        )),
    }
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
        // Quotes only; history needs a session (see `history_source`).
        ProviderKind::Stockbit => SelectedProvider {
            kind: ProviderKind::Stockbit,
            market: mock.unwrap_or_else(|| Box::new(stockbit::StockbitProvider::new())),
            profile: None,
            financials: None,
            earnings: None,
            sentiment: None,
            insights: None,
            news: None,
            screener: None,
        },
    }
}

fn msn_capability_error(subject: &str) -> IdxError {
    IdxError::Unsupported(format!("{subject}: command requires --provider msn"))
}

pub fn broker_provider() -> Box<dyn BrokerSummaryProvider> {
    #[cfg(feature = "testing")]
    if mock::enabled() {
        return Box::new(mock::MockBrokerProvider);
    }
    Box::new(ipot::IpotProvider::new())
}

pub fn trading_summary_provider() -> Box<dyn TradingSummaryProvider> {
    #[cfg(feature = "testing")]
    if mock::enabled() {
        return Box::new(mock::MockTradingSummaryProvider);
    }
    Box::new(idx::IdxSiteProvider)
}

pub fn intraday_provider() -> Box<dyn IntradayProvider> {
    #[cfg(feature = "testing")]
    if mock::enabled() {
        return Box::new(mock::MockStockbitProvider);
    }
    Box::new(stockbit::StockbitProvider::new())
}

pub fn orderbook_provider() -> Box<dyn OrderBookProvider> {
    #[cfg(feature = "testing")]
    if mock::enabled() {
        return Box::new(mock::MockStockbitProvider);
    }
    Box::new(stockbit::StockbitProvider::new())
}

/// Today in WIB, the date IDX trades on.
pub(crate) fn today_wib() -> chrono::NaiveDate {
    #[cfg(feature = "testing")]
    if let Some(today) = mock::today() {
        return today;
    }
    chrono::Utc::now().with_timezone(&idx_offset()).date_naive()
}

/// The provider that serves history for `provider` under `history_mode`.
/// `auto` uses Stockbit when it is the selected provider, the user has a
/// session and the window is one of its daily charts; otherwise Yahoo, whose
/// candles are full OHLCV for any window.
pub fn history_source(
    provider: ProviderKind,
    history_mode: HistoryProviderKind,
    period: &Period,
    interval: &Interval,
    stockbit_session: bool,
) -> ProviderKind {
    match history_mode {
        HistoryProviderKind::Yahoo => ProviderKind::Yahoo,
        HistoryProviderKind::Msn => ProviderKind::Msn,
        HistoryProviderKind::Stockbit => ProviderKind::Stockbit,
        HistoryProviderKind::Auto => {
            if provider == ProviderKind::Stockbit
                && stockbit_session
                && stockbit::exodus::history_window(period, interval).is_some()
            {
                ProviderKind::Stockbit
            } else {
                ProviderKind::Yahoo
            }
        }
    }
}

/// The history provider for a source from `history_source`.
pub fn history_provider(source: ProviderKind, verbose: bool) -> Box<dyn HistoryProvider> {
    if mock_enabled()
        && let Some(mock) = mock_history(source, verbose)
    {
        return mock;
    }
    match source {
        ProviderKind::Yahoo => Box::new(yahoo::YahooProvider::new(verbose)),
        ProviderKind::Msn => Box::new(msn::MsnProvider::new(verbose)),
        ProviderKind::Stockbit => Box::new(stockbit::StockbitProvider::new()),
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
    fn rejects_symbols_that_are_not_tickers() {
        for bad in [
            "../../REPORT",
            "a/b",
            "a\\b",
            "BB CA",
            "BBCA..JK",
            "BBCA\u{0}",
            &"A".repeat(40),
        ] {
            assert!(
                matches!(resolve_symbol(bad, "JK"), Err(IdxError::InvalidInput(_))),
                "{bad:?} accepted"
            );
        }
        assert_eq!(resolve_symbol("^jkse", "JK").unwrap(), "^JKSE.JK");
        assert_eq!(resolve_symbol("BRK-B.US", "JK").unwrap(), "BRK-B.US");
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

    #[test]
    fn auto_history_uses_stockbit_only_with_a_session_and_a_daily_window() {
        use super::history_source;
        use crate::api::types::{Interval, Period};
        use crate::config::HistoryProviderKind::{Auto, Stockbit, Yahoo};

        let source = |provider, mode, period, interval, session| {
            history_source(provider, mode, &period, &interval, session)
        };
        let year = (Period::OneYear, Interval::Day);
        assert_eq!(
            source(
                ProviderKind::Stockbit,
                Auto,
                year.0.clone(),
                year.1.clone(),
                true
            ),
            ProviderKind::Stockbit
        );
        // No session, a window Stockbit charts weekly, or another provider.
        assert_eq!(
            source(
                ProviderKind::Stockbit,
                Auto,
                year.0.clone(),
                year.1.clone(),
                false
            ),
            ProviderKind::Yahoo
        );
        assert_eq!(
            source(
                ProviderKind::Stockbit,
                Auto,
                Period::FiveYears,
                Interval::Day,
                true
            ),
            ProviderKind::Yahoo
        );
        assert_eq!(
            source(
                ProviderKind::Msn,
                Auto,
                year.0.clone(),
                year.1.clone(),
                true
            ),
            ProviderKind::Yahoo
        );
        // An explicit choice wins either way.
        assert_eq!(
            source(
                ProviderKind::Msn,
                Stockbit,
                year.0.clone(),
                year.1.clone(),
                false
            ),
            ProviderKind::Stockbit
        );
        assert_eq!(
            source(ProviderKind::Stockbit, Yahoo, year.0, year.1, true),
            ProviderKind::Yahoo
        );
    }
}
