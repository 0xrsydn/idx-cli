use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::IdxError;

use super::parse::{KeyRatios, MsnChart, MsnQuote};
use super::symbols::resolve_msn_id;

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

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        symbol: &str,
        endpoint: &str,
    ) -> Result<T, IdxError> {
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
            Ok(ok) => ok
                .into_body()
                .read_json::<T>()
                .map_err(|e| IdxError::ParseError(format!("msn {endpoint}: {e}"))),
            Err(ureq::Error::StatusCode(404)) => Err(IdxError::SymbolNotFound(symbol.to_string())),
            Err(ureq::Error::StatusCode(429)) => Err(IdxError::RateLimited),
            Err(err) => Err(IdxError::Http(format!("msn {endpoint}: {err}"))),
        }
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

    pub(super) fn fetch_charts(
        &self,
        symbol: &str,
        chart_type: &str,
    ) -> Result<Vec<MsnChart>, IdxError> {
        let id =
            resolve_msn_id(symbol).ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
        let url = format!(
            "{MSN_ASSETS_BASE_URL}Finance/Charts?apikey={MSN_API_KEY}&cm=id-id&ids={id}&type={chart_type}&wrapodata=false"
        );
        self.get_json(&url, symbol, "chart")
    }
}
