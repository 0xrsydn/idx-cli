//! Bing Finance institutional ownership HTTP client.
//!
//! Provides access to 5 ownership signal endpoints from the Bing hedge fund data
//! provider API. Each endpoint returns a list of institutional holders grouped by
//! signal type (holders, buyers, sellers, new positions, exits).
//!
//! # Usage
//! ```rust,ignore
//! use crate::api::msn::bing::{BingEndpoint, fetch_all_ownership};
//!
//! // Fetch all 5 signals for BBCA (instrument ID bn91jc)
//! let results = fetch_all_ownership("bn91jc", false)?;
//! for (signal, holders) in results {
//!     println!("{:?}: {} rows", signal, holders.len());
//! }
//! ```

use std::time::Duration;

use crate::error::IdxError;
use crate::ownership::types::{BingHolderRaw, FlowSignal};

/// Base URL for the Bing hedge fund data provider API.
const BING_OWNERSHIP_BASE: &str =
    "https://services.bingapis.com/contentservices-finance.hedgefunddataprovider/api/v1";

/// Browser-like User-Agent matching existing MSN client pattern.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

// ── Endpoint enum ────────────────────────────────────────────────────────────

/// Bing ownership API endpoint variants.
///
/// Each variant maps to one Bing hedge-fund endpoint path and the
/// [`FlowSignal`] it represents in the domain model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BingEndpoint {
    /// Existing top institutional holders.
    TopShareHolders,
    /// Net buyers over the most recent reporting period.
    TopBuyers,
    /// Net sellers over the most recent reporting period.
    TopSellers,
    /// Institutions that opened a new position this period.
    TopNewShareHolders,
    /// Institutions that fully exited their position this period.
    TopExitedShareHolders,
}

impl BingEndpoint {
    /// URL path segment used in the Bing API request.
    ///
    /// The full URL is constructed as:
    /// `{BING_OWNERSHIP_BASE}/{path}/{instrument_id}`
    pub fn path(&self) -> &'static str {
        match self {
            Self::TopShareHolders => "GetSecurityTopShareHolders",
            Self::TopBuyers => "GetSecurityTopBuyers",
            Self::TopSellers => "GetSecurityTopSellers",
            Self::TopNewShareHolders => "GetSecurityTopNewShareHolders",
            Self::TopExitedShareHolders => "GetSecurityTopExitedShareHolders",
        }
    }

    /// The [`FlowSignal`] this endpoint represents.
    pub fn signal(&self) -> FlowSignal {
        match self {
            Self::TopShareHolders => FlowSignal::Holder,
            Self::TopBuyers => FlowSignal::Buyer,
            Self::TopSellers => FlowSignal::Seller,
            Self::TopNewShareHolders => FlowSignal::NewPosition,
            Self::TopExitedShareHolders => FlowSignal::Exited,
        }
    }

    /// All endpoint variants in a canonical order.
    pub fn all() -> &'static [BingEndpoint] {
        &[
            Self::TopShareHolders,
            Self::TopBuyers,
            Self::TopSellers,
            Self::TopNewShareHolders,
            Self::TopExitedShareHolders,
        ]
    }
}

// ── HTTP helpers ─────────────────────────────────────────────────────────────

/// Build a `ureq::Agent` with timeouts matching existing MSN client conventions.
fn build_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_body(Some(Duration::from_secs(10)))
        .build()
        .into()
}

/// Wrapper type used to handle both bare-array and wrapped-object responses
/// from the Bing API. Some endpoints may return `[...]` directly, others may
/// wrap in `{ "value": [...] }` or similar.  We first try bare Vec, then
/// fall back to the wrapper.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum BingResponse {
    /// Direct JSON array of holder objects.
    Array(Vec<BingHolderRaw>),
    /// Object wrapper with a nested array under a common key.
    Wrapped(BingWrappedResponse),
}

#[derive(Debug, serde::Deserialize)]
struct BingWrappedResponse {
    /// `value` key used by some Bing OData-style responses.
    #[serde(alias = "value", alias = "Value", alias = "data", alias = "Data")]
    items: Option<Vec<BingHolderRaw>>,
}

impl BingResponse {
    fn into_holders(self) -> Vec<BingHolderRaw> {
        match self {
            BingResponse::Array(v) => v,
            BingResponse::Wrapped(w) => w.items.unwrap_or_default(),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetch ownership data from a single Bing endpoint.
///
/// `instrument_id` is the MSN instrument identifier (for example `bn91jc` for BBCA).
/// Use [`crate::api::msn::symbols::resolve_msn_id`] to obtain this from a ticker code.
///
/// Empty responses (valid HTTP 200 with empty array) are returned as `Ok(vec![])`.
/// This is expected for some IDX stocks that Bing does not cover.
///
/// # Errors
/// Returns [`IdxError::Http`] on network or non-retriable HTTP errors.
/// Returns [`IdxError::RateLimited`] after exhausting retries on 429.
/// Returns [`IdxError::ParseError`] if the response body cannot be deserialized.
pub fn fetch_holders(
    instrument_id: &str,
    endpoint: &BingEndpoint,
    verbose: bool,
) -> Result<Vec<BingHolderRaw>, IdxError> {
    let url = format!("{BING_OWNERSHIP_BASE}/{}/{instrument_id}", endpoint.path());

    if verbose {
        eprintln!("[bing] GET {url}");
    }

    let agent = build_agent();
    let endpoint_name = endpoint.path();
    let mut wait = Duration::from_millis(500);

    for attempt in 0..3 {
        let response = agent
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Origin", "https://www.bing.com")
            .header("Referer", "https://www.bing.com/")
            .call();

        match response {
            Ok(ok) => {
                let body = ok
                    .into_body()
                    .read_to_string()
                    .map_err(|e| IdxError::Http(format!("bing {endpoint_name}: read body: {e}")))?;

                if body.trim().is_empty() || body.trim() == "null" {
                    return Ok(vec![]);
                }

                let parsed: BingResponse = serde_json::from_str(&body)
                    .map_err(|e| IdxError::ParseError(format!("bing {endpoint_name}: {e}")))?;

                return Ok(parsed.into_holders());
            }
            Err(ureq::Error::StatusCode(404)) => {
                // Instrument not found on Bing — return empty, not an error.
                return Ok(vec![]);
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
                return Err(IdxError::Http(format!(
                    "bing {endpoint_name}: status {code}"
                )));
            }
            Err(err) => {
                return Err(IdxError::Http(format!("bing {endpoint_name}: {err}")));
            }
        }
    }

    Err(IdxError::RateLimited)
}

/// Fetch all 5 ownership endpoints for a given MSN instrument ID.
///
/// Returns results grouped by [`FlowSignal`] in canonical endpoint order:
/// `Holder`, `Buyer`, `Seller`, `NewPosition`, `Exited`.
///
/// Endpoints that return empty data are still included as `(signal, vec![])`.
///
/// # Errors
/// Propagates any non-empty error from [`fetch_holders`].
pub fn fetch_all_ownership(
    instrument_id: &str,
    verbose: bool,
) -> Result<Vec<(FlowSignal, Vec<BingHolderRaw>)>, IdxError> {
    let mut results = Vec::with_capacity(5);
    for endpoint in BingEndpoint::all() {
        let holders = fetch_holders(instrument_id, endpoint, verbose)?;
        results.push((endpoint.signal(), holders));
    }
    Ok(results)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── BingEndpoint::path() ──────────────────────────────────────────────

    #[test]
    fn test_endpoint_path_top_share_holders() {
        assert_eq!(
            BingEndpoint::TopShareHolders.path(),
            "GetSecurityTopShareHolders"
        );
    }

    #[test]
    fn test_endpoint_path_top_buyers() {
        assert_eq!(BingEndpoint::TopBuyers.path(), "GetSecurityTopBuyers");
    }

    #[test]
    fn test_endpoint_path_top_sellers() {
        assert_eq!(BingEndpoint::TopSellers.path(), "GetSecurityTopSellers");
    }

    #[test]
    fn test_endpoint_path_top_new_share_holders() {
        assert_eq!(
            BingEndpoint::TopNewShareHolders.path(),
            "GetSecurityTopNewShareHolders"
        );
    }

    #[test]
    fn test_endpoint_path_top_exited_share_holders() {
        assert_eq!(
            BingEndpoint::TopExitedShareHolders.path(),
            "GetSecurityTopExitedShareHolders"
        );
    }

    // ── BingEndpoint::signal() ────────────────────────────────────────────

    #[test]
    fn test_endpoint_signal_mapping() {
        assert_eq!(BingEndpoint::TopShareHolders.signal(), FlowSignal::Holder);
        assert_eq!(BingEndpoint::TopBuyers.signal(), FlowSignal::Buyer);
        assert_eq!(BingEndpoint::TopSellers.signal(), FlowSignal::Seller);
        assert_eq!(
            BingEndpoint::TopNewShareHolders.signal(),
            FlowSignal::NewPosition
        );
        assert_eq!(
            BingEndpoint::TopExitedShareHolders.signal(),
            FlowSignal::Exited
        );
    }

    // ── BingEndpoint::all() ───────────────────────────────────────────────

    #[test]
    fn test_endpoint_all_has_five_variants() {
        assert_eq!(BingEndpoint::all().len(), 5);
    }

    #[test]
    fn test_endpoint_all_covers_all_signals() {
        use std::collections::HashSet;
        let signals: HashSet<String> = BingEndpoint::all()
            .iter()
            .map(|e| format!("{:?}", e.signal()))
            .collect();
        assert!(signals.contains("Holder"));
        assert!(signals.contains("Buyer"));
        assert!(signals.contains("Seller"));
        assert!(signals.contains("NewPosition"));
        assert!(signals.contains("Exited"));
    }

    // ── Fixture deserialization ───────────────────────────────────────────

    #[test]
    fn test_deserialize_holders_fixture() {
        let json = include_str!("../../../tests/fixtures/bing_holders_bbca.json");
        let holders: Vec<BingHolderRaw> = serde_json::from_str(json)
            .expect("bing_holders_bbca.json should deserialize into Vec<BingHolderRaw>");
        assert!(
            !holders.is_empty(),
            "fixture should have at least one holder"
        );
        // First holder should have a name
        assert!(
            holders[0].investor_name.is_some(),
            "first holder should have investor_name"
        );
    }

    #[test]
    fn test_deserialize_buyers_fixture() {
        let json = include_str!("../../../tests/fixtures/bing_buyers_bbca.json");
        let holders: Vec<BingHolderRaw> = serde_json::from_str(json)
            .expect("bing_buyers_bbca.json should deserialize into Vec<BingHolderRaw>");
        assert!(
            !holders.is_empty(),
            "fixture should have at least one buyer"
        );
    }

    // ── Empty response handling ───────────────────────────────────────────

    #[test]
    fn test_deserialize_empty_array_is_ok() {
        let json = "[]";
        let holders: Vec<BingHolderRaw> = serde_json::from_str(json).expect("empty array is valid");
        assert!(holders.is_empty());
    }

    #[test]
    fn test_bing_response_bare_array() {
        let json = r#"[
            {
                "investorName": "Vanguard Group",
                "investorType": "Institutional",
                "sharesHeld": 1234567.0,
                "reportDate": "2024-12-31"
            }
        ]"#;
        let resp: BingResponse = serde_json::from_str(json).unwrap();
        let holders = resp.into_holders();
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].investor_name.as_deref(), Some("Vanguard Group"));
    }

    #[test]
    fn test_bing_response_wrapped_value_key() {
        let json = r#"{
            "value": [
                {
                    "InvestorName": "BlackRock",
                    "InvestorType": "Institutional",
                    "SharesHeld": 9876543.0
                }
            ]
        }"#;
        let resp: BingResponse = serde_json::from_str(json).unwrap();
        let holders = resp.into_holders();
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].investor_name.as_deref(), Some("BlackRock"));
    }

    #[test]
    fn test_bing_response_wrapped_empty_value() {
        let json = r#"{ "value": [] }"#;
        let resp: BingResponse = serde_json::from_str(json).unwrap();
        let holders = resp.into_holders();
        assert!(holders.is_empty());
    }

    #[test]
    fn test_holder_optional_fields_are_none() {
        let json = r#"[{"investorName": "Some Fund"}]"#;
        let holders: Vec<BingHolderRaw> = serde_json::from_str(json).unwrap();
        assert_eq!(holders.len(), 1);
        assert!(holders[0].shares_held.is_none());
        assert!(holders[0].shares_changed.is_none());
        assert!(holders[0].pct_outstanding.is_none());
        assert!(holders[0].value.is_none());
        assert!(holders[0].report_date.is_none());
    }
}
