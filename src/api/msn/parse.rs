use super::map::{parse_fundamentals, parse_quote};
use super::raw_types::{KeyRatios, MsnQuote};
use crate::api::types::{Fundamentals, Quote};
use crate::error::IdxError;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> {
    let quotes: Vec<MsnQuote> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    parse_quote(symbol, &quotes)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_fundamentals_from_str(
    raw: &str,
    quote_raw: Option<&str>,
) -> Result<Fundamentals, IdxError> {
    let ratios: Vec<KeyRatios> =
        serde_json::from_str(raw).map_err(|e| IdxError::ParseError(e.to_string()))?;
    let quote = quote_raw
        .map(serde_json::from_str::<Vec<MsnQuote>>)
        .transpose()
        .map_err(|e| IdxError::ParseError(e.to_string()))?
        .and_then(|quotes| quotes.into_iter().next());
    parse_fundamentals(&ratios, quote.as_ref())
}

#[cfg_attr(not(test), allow(dead_code))]
#[allow(dead_code)]
#[cfg(test)]
mod tests {
    use super::{parse_fundamentals_from_str, parse_quote_from_str};
    use crate::api::types::Period;

    #[test]
    fn parses_quote_fixture_json() {
        let raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .expect("quote fixture exists");
        let quote = parse_quote_from_str("BBCA.JK", &raw).expect("quote parsed");
        assert_eq!(quote.symbol, "BBCA.JK");
        assert_eq!(quote.price, 9875);
        assert_eq!(quote.change, 117);
        assert_eq!(quote.volume, 12_300_000);
        assert_eq!(quote.market_cap, Some(1_215_200_000_000_000));
        assert_eq!(quote.avg_volume, Some(10_000_000));
    }

    #[test]
    fn parses_fundamentals_fixture_json() {
        let raw = std::fs::read_to_string("tests/fixtures/msn_keyratios_bbca.json")
            .expect("fundamentals fixture exists");
        let quote_raw = std::fs::read_to_string("tests/fixtures/msn_quote_bbca.json")
            .expect("quote fixture exists");
        let fundamentals =
            parse_fundamentals_from_str(&raw, Some(&quote_raw)).expect("fundamentals parsed");
        assert_eq!(fundamentals.trailing_pe, Some(25.4));
        assert_eq!(fundamentals.price_to_book, Some(4.6));
        assert_eq!(fundamentals.return_on_equity, Some(0.198));
        assert_eq!(fundamentals.revenue_growth, Some(0.081));
        assert_eq!(fundamentals.earnings_growth, Some(0.121));
        assert_eq!(fundamentals.market_cap, Some(1_215_200_000_000_000));
    }
}
