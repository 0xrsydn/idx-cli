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
        Box::new(MockProvider)
    } else {
        Box::new(yahoo::YahooProvider::new())
    }
}

struct MockProvider;

impl MarketDataProvider for MockProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError> {
        Ok(Quote {
            symbol: symbol.to_string(),
            price: 9875.0,
            change: 117.0,
            change_pct: 1.2,
            volume: 12_300_000,
            market_cap: Some(1_215_200_000_000_000.0),
            week52_high: Some(10_250.0),
            week52_low: Some(7_800.0),
            week52_position: Some(0.73),
            range_signal: Some("upper".to_string()),
            prev_close: Some(9_758.0),
            avg_volume: Some(10_000_000),
        })
    }

    fn history(
        &self,
        _symbol: &str,
        _period: &Period,
        _interval: &Interval,
    ) -> Result<Vec<Ohlc>, IdxError> {
        Ok(vec![Ohlc {
            date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).expect("valid date"),
            open: 9800.0,
            high: 9900.0,
            low: 9750.0,
            close: 9875.0,
            volume: 12_300_000,
        }])
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
    }
}
