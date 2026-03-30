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
    let output = Command::new(&binary).args(args).output().map_err(|e| {
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
