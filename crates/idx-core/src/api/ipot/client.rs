use std::time::Duration;

use crate::error::IdxError;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
/// Tries per request. In some stretches of seconds the server answers its
/// own nginx 404 page to a third or more of requests and then recovers, so a
/// 404 is retried (an unknown ticker is a 200, not a 404).
const ATTEMPTS: u32 = 5;
const FIRST_PAUSE: Duration = Duration::from_millis(250);

pub(super) struct IpotClient {
    agent: ureq::Agent,
}

impl IpotClient {
    pub(super) fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            // A server that stalls before sending headers must not hold a
            // call (and an MCP call slot) forever.
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        Self { agent }
    }

    pub(super) fn fetch_html(&self, url: &str) -> Result<String, IdxError> {
        let mut pause = FIRST_PAUSE;
        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            if attempt > 1 {
                std::thread::sleep(pause);
                pause *= 2;
            }
            let response = self
                .agent
                .get(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "text/html")
                .call();
            match response {
                Ok(ok) => {
                    return ok
                        .into_body()
                        .read_to_string()
                        .map_err(|e| IdxError::Http(format!("ipot broker summary: {e}")));
                }
                Err(ureq::Error::StatusCode(code)) if code == 404 || code >= 500 => {
                    last = Some(code);
                }
                Err(ureq::Error::StatusCode(429)) => return Err(IdxError::RateLimited),
                Err(err) => return Err(IdxError::Http(format!("ipot broker summary: {err}"))),
            }
        }
        Err(IdxError::Http(format!(
            "ipot broker summary: status {} after {ATTEMPTS} attempts",
            last.unwrap_or_default()
        )))
    }
}
