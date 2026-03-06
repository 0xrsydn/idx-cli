use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::Duration;

use crate::api::types::{Interval, Period};
use crate::error::IdxError;

use super::raw_types::{ChartResponse, QuoteSummaryResponse};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const BASE_URL: &str = "https://query2.finance.yahoo.com";
const COOKIE_FETCH_URL: &str = "https://fc.yahoo.com";
const CRUMB_FETCH_URL: &str = "https://query1.finance.yahoo.com/v1/test/getcrumb";

pub(super) struct YahooClient {
    agent: ureq::Agent,
    crumb: Mutex<Option<String>>,
}

impl YahooClient {
    pub(super) fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .build()
            .into();

        Self {
            agent,
            crumb: Mutex::new(None),
        }
    }

    fn chart_url(symbol: &str, period: &Period, interval: &Interval) -> String {
        format!(
            "{BASE_URL}/v8/finance/chart/{symbol}?range={}&interval={}",
            period.as_str(),
            interval.as_str()
        )
    }

    fn quote_summary_url(symbol: &str, crumb: &str) -> String {
        format!(
            "{BASE_URL}/v10/finance/quoteSummary/{symbol}?modules=summaryDetail,defaultKeyStatistics,financialData,assetProfile,incomeStatementHistory&crumb={crumb}"
        )
    }

    fn cookie_jar_path() -> PathBuf {
        PathBuf::from(format!("/tmp/idx_yf_{}.txt", std::process::id()))
    }

    pub(super) fn parse_crumb_body(raw: &str) -> Result<String, IdxError> {
        let crumb = raw.trim();
        if crumb.is_empty() {
            return Err(IdxError::Http("received empty Yahoo crumb".to_string()));
        }

        let normalized = crumb.to_ascii_lowercase();
        if normalized.contains("<html>") || normalized.contains("<!doctype html") {
            return Err(IdxError::Http(
                "received HTML instead of a Yahoo crumb".to_string(),
            ));
        }
        if normalized.contains("too many requests") {
            return Err(IdxError::Http(
                "Yahoo crumb request was rate limited".to_string(),
            ));
        }

        Ok(crumb.to_string())
    }

    fn cookie_header_from_jar(path: &Path) -> Result<String, IdxError> {
        let jar = std::fs::read_to_string(path).map_err(|e| {
            IdxError::Http(format!(
                "failed to read Yahoo cookie jar {}: {e}",
                path.display()
            ))
        })?;

        let cookies: Vec<String> = jar
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }

                let candidate = trimmed.strip_prefix("#HttpOnly_").unwrap_or(trimmed);
                if candidate.starts_with('#') {
                    return None;
                }

                let fields: Vec<_> = candidate.split('\t').collect();
                if fields.len() < 7 {
                    return None;
                }

                let name = fields[5].trim();
                let value = fields[6].trim();
                if name.is_empty() {
                    return None;
                }

                Some(format!("{name}={value}"))
            })
            .collect();

        if cookies.is_empty() {
            return Err(IdxError::Http(format!(
                "Yahoo cookie jar {} did not contain any cookies",
                path.display()
            )));
        }

        Ok(cookies.join("; "))
    }

    fn chrome_curl_binary() -> Option<&'static str> {
        // curl-impersonate-chrome ships per-version binaries (curl_chrome131 etc).
        // Try latest versions first; no --impersonate flag needed; the binary is the impersonation.
        const CANDIDATES: &[&str] = &[
            "curl_chrome136",
            "curl_chrome133a",
            "curl_chrome131",
            "curl_chrome124",
            "curl_chrome120",
            "curl_chrome116",
        ];
        CANDIDATES
            .iter()
            .copied()
            .find(|bin| Command::new(bin).arg("--version").output().is_ok())
    }

    fn run_curl(stage: &str, binary: &str, args: &[&str]) -> Result<Output, IdxError> {
        let output = Command::new(binary).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                return IdxError::Http(format!(
                    "curl-impersonate binary '{binary}' not found; install nixpkgs#curl-impersonate-chrome"
                ));
            }
            IdxError::Http(format!("failed to run {binary} for Yahoo {stage}: {e}"))
        })?;

        if output.status.success() {
            return Ok(output);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        Err(IdxError::Http(format!(
            "Yahoo {stage} {binary} failed (status {}): {}",
            output.status,
            if detail.is_empty() {
                "no output"
            } else {
                detail
            }
        )))
    }

    fn fetch_crumb_via_curl(&self) -> Result<String, IdxError> {
        let binary = Self::chrome_curl_binary().ok_or_else(|| {
            IdxError::Http(
                "no curl_chrome* binary found; install nixpkgs#curl-impersonate-chrome".to_string(),
            )
        })?;

        let cookie_jar = Self::cookie_jar_path();
        let cookie_jar_str = cookie_jar.to_str().ok_or_else(|| {
            IdxError::Io(format!(
                "failed to encode Yahoo cookie jar path {}",
                cookie_jar.display()
            ))
        })?;

        // Step 1: fetch fc.yahoo.com to set A3 cookie (returns 404 but writes cookie jar).
        // We allow non-zero exit here since 404 still writes the cookie.
        let _ = Command::new(binary)
            .args([
                "--silent",
                "--cookie-jar",
                cookie_jar_str,
                COOKIE_FETCH_URL,
                "--output",
                "/dev/null",
            ])
            .output();

        // Step 2: fetch crumb with cookie jar (Chrome TLS fingerprint + A3 cookie).
        let output = Self::run_curl(
            "crumb fetch",
            binary,
            &["--silent", "--cookie", cookie_jar_str, CRUMB_FETCH_URL],
        )?;

        let body = String::from_utf8_lossy(&output.stdout);
        Self::parse_crumb_body(&body)
    }

    fn get_or_init_crumb(&self) -> Result<String, IdxError> {
        let mut guard = self
            .crumb
            .lock()
            .map_err(|e| IdxError::Io(format!("crumb lock poisoned: {e}")))?;

        if let Some(crumb) = guard.as_ref() {
            return Ok(crumb.clone());
        }

        let crumb = self.fetch_crumb_via_curl()?;
        *guard = Some(crumb.clone());
        Ok(crumb)
    }

    fn clear_crumb(&self) -> Result<(), IdxError> {
        let mut guard = self
            .crumb
            .lock()
            .map_err(|e| IdxError::Io(format!("crumb lock poisoned: {e}")))?;
        *guard = None;
        Ok(())
    }

    pub(super) fn fetch_chart(
        &self,
        symbol: &str,
        period: &Period,
        interval: &Interval,
    ) -> Result<ChartResponse, IdxError> {
        let mut wait = Duration::from_millis(250);
        for attempt in 0..3 {
            let url = Self::chart_url(symbol, period, interval);
            let response = self.agent.get(&url).header("User-Agent", USER_AGENT).call();
            match response {
                Ok(ok) => {
                    return ok
                        .into_body()
                        .read_json::<ChartResponse>()
                        .map_err(|e| IdxError::ParseError(e.to_string()));
                }
                Err(ureq::Error::StatusCode(429)) => {
                    if attempt < 2 {
                        std::thread::sleep(wait + jitter());
                        wait *= 2;
                    }
                }
                Err(ureq::Error::StatusCode(404)) => {
                    return Err(IdxError::SymbolNotFound(symbol.to_string()));
                }
                Err(e) => return Err(IdxError::Http(e.to_string())),
            }
        }
        Err(IdxError::RateLimited)
    }

    pub(super) fn fetch_quote_summary(
        &self,
        symbol: &str,
    ) -> Result<QuoteSummaryResponse, IdxError> {
        for auth_attempt in 0..2 {
            let crumb = self.get_or_init_crumb()?;
            let cookie_header = match Self::cookie_header_from_jar(&Self::cookie_jar_path()) {
                Ok(header) => header,
                Err(err) => {
                    eprintln!("warning: failed to parse Yahoo cookie jar: {err}");
                    return Err(IdxError::AuthError(format!(
                        "failed to parse Yahoo cookies: {err}"
                    )));
                }
            };
            let url = Self::quote_summary_url(symbol, &crumb);
            let mut wait = Duration::from_millis(250);

            for attempt in 0..3 {
                let mut req = self.agent.get(&url).header("User-Agent", USER_AGENT);
                if !cookie_header.is_empty() {
                    req = req.header("Cookie", &cookie_header);
                }
                let response = req.call();
                match response {
                    Ok(ok) => {
                        return ok
                            .into_body()
                            .read_json::<QuoteSummaryResponse>()
                            .map_err(|e| IdxError::ParseError(e.to_string()));
                    }
                    Err(ureq::Error::StatusCode(401)) => {
                        if auth_attempt == 0 {
                            self.clear_crumb()?;
                            break;
                        }
                        return Err(IdxError::Http(
                            "yahoo quoteSummary returned unauthorized (401)".to_string(),
                        ));
                    }
                    Err(ureq::Error::StatusCode(429)) => {
                        if attempt < 2 {
                            std::thread::sleep(wait + jitter());
                            wait *= 2;
                        }
                    }
                    Err(ureq::Error::StatusCode(404)) => {
                        return Err(IdxError::SymbolNotFound(symbol.to_string()));
                    }
                    Err(e) => return Err(IdxError::Http(e.to_string())),
                }
            }
        }

        Err(IdxError::RateLimited)
    }
}

fn jitter() -> Duration {
    Duration::from_millis(fastrand::u64(0..100))
}

#[cfg(test)]
mod tests {
    use super::YahooClient;

    #[test]
    fn parses_crumb_body_trimmed() {
        let crumb = YahooClient::parse_crumb_body("  abc123xyz\n").expect("crumb should parse");
        assert_eq!(crumb, "abc123xyz");

        let empty = YahooClient::parse_crumb_body(" \n").expect_err("empty crumb must fail");
        assert!(matches!(empty, crate::error::IdxError::Http(_)));

        let html = YahooClient::parse_crumb_body("<html>blocked</html>")
            .expect_err("html crumb must fail");
        assert!(matches!(html, crate::error::IdxError::Http(_)));

        let rate_limited = YahooClient::parse_crumb_body("Too Many Requests")
            .expect_err("rate limited crumb must fail");
        assert!(matches!(rate_limited, crate::error::IdxError::Http(_)));
    }
}
