pub mod types;
pub mod yahoo;

use crate::error::IdxError;
use types::{Interval, Ohlc, Period, Quote};

pub trait MarketDataProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError>;
    fn history(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError>;
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

pub fn default_provider() -> Box<dyn MarketDataProvider> {
    if std::env::var("IDX_USE_MOCK_PROVIDER").is_ok() {
        Box::new(MockProvider::from_fixtures())
    } else {
        Box::new(yahoo::YahooProvider::new())
    }
}

pub struct MockProvider {
    quote: Result<Quote, IdxError>,
    history: Result<Vec<Ohlc>, IdxError>,
}

impl MockProvider {
    pub fn from_fixtures() -> Self {
        if std::env::var("IDX_MOCK_ERROR").is_ok() {
            return Self::with_error(IdxError::ProviderUnavailable);
        }

        let quote_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_1d.json")
            .unwrap_or_else(|_| "{}".to_string());
        let history_raw = std::fs::read_to_string("tests/fixtures/chart_bbca_3mo.json")
            .unwrap_or_else(|_| "{}".to_string());

        let quote = yahoo::parse_quote_from_str("BBCA.JK", &quote_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));
        let history = yahoo::parse_history_from_str(&history_raw)
            .map_err(|e| IdxError::ParseError(e.to_string()));

        Self { quote, history }
    }

    pub fn with_error(err: IdxError) -> Self {
        Self {
            quote: Err(err),
            history: Err(IdxError::ProviderUnavailable),
        }
    }
}

impl MarketDataProvider for MockProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        let mut q = self.quote.clone()?;
        q.symbol = symbol.to_string();
        Ok(q)
    }

    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
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
