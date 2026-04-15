use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::IdxError;

use super::raw_types::{
    KeyRatios, MsnQuote, RawChartResponse, RawEarningsResponse, RawEquity, RawFinancialStatement,
    RawInsight, RawNewsFeed, RawScreenerResponse, RawSentiment, ScreenerFilter, ScreenerOrder,
    ScreenerRequest,
};
use super::symbols::resolve_msn_id;
use crate::api::types::{Interval, Period};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const MSN_ASSETS_BASE_URL: &str = "https://assets.msn.com/service/";
const MSN_API_BASE_URL: &str = "https://api.msn.com/msn/v0/pages/finance/";
// Public API key from MSN Money website (embedded in frontend JS)
const MSN_API_KEY: &str = "0QfOX3Vn51YCzitbLaRkTTBadtWpgTN8NZLW0C1SEM";

pub(super) struct MsnClient {
    agent: ureq::Agent,
}

impl MsnClient {
    pub(super) fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .build()
            .into();

        Self { agent }
    }

    fn mock_error() -> Option<IdxError> {
        if std::env::var("IDX_USE_MOCK_PROVIDER").is_ok() && std::env::var("IDX_MOCK_ERROR").is_ok()
        {
            Some(IdxError::ProviderUnavailable)
        } else {
            None
        }
    }

    fn mock_body(endpoint: &str) -> Option<&'static str> {
        if std::env::var("IDX_USE_MOCK_PROVIDER").is_err() {
            return None;
        }

        Some(match endpoint {
            "quote" => include_str!("../../../tests/fixtures/msn_quote_bbca.json"),
            "keyratios" => include_str!("../../../tests/fixtures/msn_keyratios_bbca.json"),
            "equities" => include_str!("../../../tests/fixtures/msn_profile_bbca.json"),
            "financialstatements" => {
                include_str!("../../../tests/fixtures/msn_financials_bbca.json")
            }
            "earnings" => include_str!("../../../tests/fixtures/msn_earnings_bbca.json"),
            "sentiment" => include_str!("../../../tests/fixtures/msn_sentiment_bbca.json"),
            "insights" => include_str!("../../../tests/fixtures/msn_insights_bbca.json"),
            "news" => include_str!("../../../tests/fixtures/msn_news_bbca.json"),
            "screener" => include_str!("../../../tests/fixtures/msn_screener_id_topperfs.json"),
            "chart" => include_str!("../../../tests/fixtures/msn_chart_bbca_3m.json"),
            _ => return None,
        })
    }

    fn mock_json<T: DeserializeOwned>(endpoint: &str) -> Result<Option<T>, IdxError> {
        let Some(body) = Self::mock_body(endpoint) else {
            return Ok(None);
        };

        serde_json::from_str(body)
            .map(Some)
            .map_err(|e| IdxError::ParseError(format!("msn {endpoint}: {e}")))
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        symbol: &str,
        endpoint: &str,
    ) -> Result<T, IdxError> {
        if let Some(err) = Self::mock_error() {
            return Err(err);
        }

        if let Some(mocked) = Self::mock_json(endpoint)? {
            return Ok(mocked);
        }

        let mut wait = Duration::from_millis(500);
        for attempt in 0..3 {
            let response = self
                .agent
                .get(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
                .header("Accept-Language", "en-US,en;q=0.9,id;q=0.8")
                .header("Origin", "https://www.msn.com")
                .header("Referer", "https://www.msn.com/")
                .call();

            match response {
                Ok(ok) => {
                    return ok
                        .into_body()
                        .read_json::<T>()
                        .map_err(|e| IdxError::ParseError(format!("msn {endpoint}: {e}")));
                }
                Err(ureq::Error::StatusCode(404)) => {
                    return Err(IdxError::SymbolNotFound(symbol.to_string()));
                }
                Err(ureq::Error::StatusCode(429)) => {
                    if attempt < 2 {
                        std::thread::sleep(wait);
                        wait *= 2;
                        continue;
                    }
                    return Err(IdxError::RateLimited);
                }
                Err(ureq::Error::StatusCode(code)) if code >= 500 => {
                    if attempt < 2 {
                        std::thread::sleep(wait);
                        wait *= 2;
                        continue;
                    }
                    return Err(IdxError::Http(format!("msn {endpoint}: status {code}")));
                }
                Err(err) => return Err(IdxError::Http(format!("msn {endpoint}: {err}"))),
            }
        }
        Err(IdxError::RateLimited)
    }

    fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
        symbol: &str,
        endpoint: &str,
    ) -> Result<T, IdxError> {
        if let Some(err) = Self::mock_error() {
            return Err(err);
        }

        if let Some(mocked) = Self::mock_json(endpoint)? {
            return Ok(mocked);
        }

        let mut wait = Duration::from_millis(500);
        for attempt in 0..3 {
            let response = self
                .agent
                .post(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
                .header("Accept-Language", "en-US,en;q=0.9,id;q=0.8")
                .header("Origin", "https://www.msn.com")
                .header("Referer", "https://www.msn.com/")
                .header("Content-Type", "text/plain;charset=UTF-8")
                .send_json(body);

            match response {
                Ok(ok) => {
                    return ok
                        .into_body()
                        .read_json::<T>()
                        .map_err(|e| IdxError::ParseError(format!("msn {endpoint}: {e}")));
                }
                Err(ureq::Error::StatusCode(404)) => {
                    return Err(IdxError::SymbolNotFound(symbol.to_string()));
                }
                Err(ureq::Error::StatusCode(429)) => {
                    if attempt < 2 {
                        std::thread::sleep(wait);
                        wait *= 2;
                        continue;
                    }
                    return Err(IdxError::RateLimited);
                }
                Err(ureq::Error::StatusCode(code)) if code >= 500 => {
                    if attempt < 2 {
                        std::thread::sleep(wait);
                        wait *= 2;
                        continue;
                    }
                    return Err(IdxError::Http(format!("msn {endpoint}: status {code}")));
                }
                Err(err) => return Err(IdxError::Http(format!("msn {endpoint}: {err}"))),
            }
        }
        Err(IdxError::RateLimited)
    }

    pub(super) fn fetch_quotes(&self, symbol: &str) -> Result<Vec<MsnQuote>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Quotes?apikey={MSN_API_KEY}&ids={id}&wrapodata=false"
        );
        self.get_json(&url, symbol, "quote")
    }

    pub(super) fn fetch_key_ratios(&self, symbol: &str) -> Result<Vec<KeyRatios>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url =
            format!("{MSN_API_BASE_URL}keyratios?apikey={MSN_API_KEY}&ids={id}&wrapodata=false");
        self.get_json(&url, symbol, "keyratios")
    }

    pub(super) fn fetch_equities(&self, symbol: &str) -> Result<Vec<RawEquity>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Equities?apikey={MSN_API_KEY}&ids={id}&wrapodata=false"
        );
        self.get_json(&url, symbol, "equities")
    }

    pub(super) fn fetch_financial_statements(
        &self,
        symbol: &str,
    ) -> Result<Vec<RawFinancialStatement>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Equities/financialstatements?apikey={MSN_API_KEY}&ids={id}&wrapodata=false"
        );
        self.get_json(&url, symbol, "financialstatements")
    }

    pub(super) fn fetch_earnings(&self, symbol: &str) -> Result<RawEarningsResponse, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Events/Earnings?apikey={MSN_API_KEY}&ids={id}&wrapodata=false"
        );
        self.get_json(&url, symbol, "earnings")
    }

    pub(super) fn fetch_sentiment(&self, symbol: &str) -> Result<Vec<RawSentiment>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/SentimentBrowser?apikey={MSN_API_KEY}&cm=id-id&it=web&scn=ANON&ids={id}&wrapodata=false&flightId=INeedDau"
        );
        self.get_json(&url, symbol, "sentiment")
    }

    pub(super) fn fetch_insights(&self, symbol: &str) -> Result<Vec<RawInsight>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url =
            format!("{MSN_API_BASE_URL}insights?apikey={MSN_API_KEY}&ids={id}&wrapodata=false");
        self.get_json(&url, symbol, "insights")
    }

    pub(super) fn fetch_news(&self, symbol: &str, limit: usize) -> Result<RawNewsFeed, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}MSN/Feed/me?$top={limit}&apikey={MSN_API_KEY}&cm=id-id&contentType=article,video,slideshow&it=web&query=ef_stock_{id}&queryType=entityfeed&responseSchema=cardview&scn=ANON&wrapodata=false"
        );
        self.get_json(&url, symbol, "news")
    }

    pub(super) fn fetch_screener(
        &self,
        filter: &str,
        region: &str,
        limit: usize,
    ) -> Result<RawScreenerResponse, IdxError> {
        let url =
            format!("{MSN_ASSETS_BASE_URL}Finance/Screener?apikey={MSN_API_KEY}&wrapodata=false");
        let req = ScreenerRequest {
            filter: vec![
                ScreenerFilter {
                    key: filter.to_string(),
                    key_group: "st_list_".to_string(),
                    is_range: false,
                },
                ScreenerFilter {
                    key: region.to_string(),
                    key_group: "st_reg_".to_string(),
                    is_range: false,
                },
            ],
            order: ScreenerOrder {
                key: "st_1yr_asc_order".to_string(),
                dir: "desc".to_string(),
            },
            return_value_type: vec!["quote".to_string(), "equity".to_string()],
            screener_type: "stock".to_string(),
            limit,
            page_index: 0,
        };

        self.post_json(&url, &req, "SCREENER", "screener")
    }

    pub(super) fn fetch_chart(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<Vec<RawChartResponse>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let chart_type = msn_chart_type(period, interval)?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Charts?apikey={MSN_API_KEY}&cm=id-id&ids={id}&type={chart_type}&wrapodata=false"
        );
        self.get_json(&url, symbol, "chart")
    }
}

fn msn_chart_type(period: &Period, interval: &Interval) -> Result<&'static str, IdxError> {
    if !matches!(interval, Interval::Day) {
        return Err(IdxError::Unsupported(
            "MSN charts currently support only --interval 1d for IDX history".into(),
        ));
    }

    match period {
        Period::OneMonth => Ok("1M"),
        Period::ThreeMonths => Ok("3M"),
        Period::OneYear => Ok("1Y"),
        _ => Err(IdxError::Unsupported(
            "MSN charts currently support --period 1mo, 3mo, or 1y with --interval 1d".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::msn_chart_type;
    use crate::api::types::{Interval, Period};
    use crate::error::IdxError;

    #[test]
    fn maps_supported_msn_chart_types() {
        assert_eq!(
            msn_chart_type(&Period::OneMonth, &Interval::Day).unwrap(),
            "1M"
        );
        assert_eq!(
            msn_chart_type(&Period::ThreeMonths, &Interval::Day).unwrap(),
            "3M"
        );
        assert_eq!(
            msn_chart_type(&Period::OneYear, &Interval::Day).unwrap(),
            "1Y"
        );
    }

    #[test]
    fn rejects_unsupported_msn_chart_types() {
        let err = msn_chart_type(&Period::ThreeMonths, &Interval::Week).unwrap_err();
        assert!(matches!(err, IdxError::Unsupported(_)));

        let err = msn_chart_type(&Period::SixMonths, &Interval::Day).unwrap_err();
        assert!(matches!(err, IdxError::Unsupported(_)));
    }
}
