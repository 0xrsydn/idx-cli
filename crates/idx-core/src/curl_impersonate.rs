use std::process::{Command, Output};

use crate::error::IdxError;

const CANDIDATES: &[&str] = &[
    "curl_chrome142",
    "curl_chrome136",
    "curl_chrome133a",
    "curl_chrome131",
    "curl_chrome124",
    "curl_chrome120",
    "curl_chrome116",
];
const OVERRIDE_ENV: &str = "IDX_CURL_IMPERSONATE_BIN";

pub fn chrome_curl_binary() -> Result<String, IdxError> {
    if let Ok(value) = std::env::var(OVERRIDE_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    CANDIDATES
        .iter()
        .copied()
        .find(|bin| Command::new(bin).arg("--version").output().is_ok())
        .map(str::to_string)
        .ok_or_else(|| {
            IdxError::Http(format!(
                "no curl_chrome* binary found; set {OVERRIDE_ENV} or install nixpkgs#curl-impersonate-chrome"
            ))
        })
}

pub fn run(stage: &str, args: &[&str]) -> Result<Output, IdxError> {
    let owned_args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    run_owned(stage, &owned_args)
}

pub fn run_owned(stage: &str, args: &[String]) -> Result<Output, IdxError> {
    let binary = chrome_curl_binary()?;
    run_binary(stage, &binary, args)
}

/// Browser profiles tried, in order, when a site's bot protection rejects
/// some TLS fingerprints but not others (IDX's Cloudflare rules block
/// `curl_chrome142` on some pages while Firefox/Safari profiles pass).
const FALLBACK_PROFILES: &[&str] = &[
    "curl_chrome142",
    "curl_firefox144",
    "curl_chrome136",
    "curl_firefox135",
    "curl_safari184",
    "curl_chrome131",
    "curl_chrome124",
    "curl_chrome120",
    "curl_chrome116",
];
const FALLBACK_ROUNDS: usize = 2;

/// Run curl with each available impersonation profile until `accept` approves
/// the response body. `IDX_CURL_IMPERSONATE_BIN`, when set, is the only
/// profile used. Returns the first accepted output, or the last error.
pub fn run_owned_with_fallback(
    stage: &str,
    args: &[String],
    accept: &dyn Fn(&[u8]) -> Result<(), IdxError>,
) -> Result<Output, IdxError> {
    let profiles: Vec<String> = match std::env::var(OVERRIDE_ENV) {
        Ok(value) if !value.trim().is_empty() => vec![value.trim().to_string()],
        _ => FALLBACK_PROFILES
            .iter()
            .copied()
            .filter(|bin| Command::new(bin).arg("--version").output().is_ok())
            .map(str::to_string)
            .collect(),
    };
    if profiles.is_empty() {
        return Err(IdxError::Http(format!(
            "no curl-impersonate binary found; set {OVERRIDE_ENV} or install nixpkgs#curl-impersonate"
        )));
    }

    let mut last_error = None;
    for _ in 0..FALLBACK_ROUNDS {
        for binary in &profiles {
            match run_binary(stage, binary, args) {
                Ok(output) => match accept(&output.stdout) {
                    Ok(()) => return Ok(output),
                    Err(err) => last_error = Some(err),
                },
                Err(err) => last_error = Some(err),
            }
        }
    }
    Err(last_error.expect("at least one profile was tried"))
}

fn run_binary(stage: &str, binary: &str, args: &[String]) -> Result<Output, IdxError> {
    let output = Command::new(binary).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            return IdxError::Http(format!(
                "curl-impersonate binary '{binary}' not found; set {OVERRIDE_ENV} or install nixpkgs#curl-impersonate-chrome"
            ));
        }
        IdxError::Http(format!("failed to run {binary} for {stage}: {e}"))
    })?;

    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    Err(IdxError::Http(format!(
        "{stage} {binary} failed (status {}): {}",
        output.status,
        if detail.is_empty() {
            "no output"
        } else {
            detail
        }
    )))
}
