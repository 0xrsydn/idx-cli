use std::io::Read;
use std::process::{Command, Output, Stdio};

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

/// Browser profiles tried, in order, because a site's bot protection can reject
/// some TLS fingerprints but not others. On IDX which profiles pass changes over
/// time and by path, and at times none does (on 2026-11-05 Chrome, Firefox and
/// Safari profiles all got a challenge). A profile only changes the fingerprint,
/// so it cannot solve a challenge that is active for this client.
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

/// Marks the line curl adds to stderr describing the response. `--fail` drops
/// the response itself, and stdout must stay the body alone.
const PROBE_PREFIX: &str = "idx-curl-probe";
const PROBE_WRITE_OUT: &str =
    "%{stderr}idx-curl-probe status=%{http_code} cf-mitigated=%header{cf-mitigated}\\n";

/// What the probe line says about a response.
#[derive(Debug, PartialEq, Eq)]
struct ResponseProbe {
    status: String,
    challenged: bool,
}

/// Separates the probe line from curl's own stderr. Without a probe line (the
/// caller did not ask for one) stderr comes back unchanged.
fn split_probe(stderr: &[u8]) -> (Option<ResponseProbe>, Vec<u8>) {
    let mut probe = None;
    let mut rest = Vec::with_capacity(stderr.len());
    for line in stderr.split_inclusive(|byte| *byte == b'\n') {
        let parsed = std::str::from_utf8(line)
            .ok()
            .and_then(|text| text.trim_end().strip_prefix(PROBE_PREFIX))
            .and_then(|fields| fields.strip_prefix(" status="))
            .and_then(|fields| fields.split_once(" cf-mitigated="));
        match parsed {
            Some((status, mitigated)) => {
                probe = Some(ResponseProbe {
                    status: status.to_string(),
                    challenged: mitigated.trim().eq_ignore_ascii_case("challenge"),
                });
            }
            None => rest.extend_from_slice(line),
        }
    }
    (probe, rest)
}

/// Run curl with each available impersonation profile until `accept` approves
/// the response body. `IDX_CURL_IMPERSONATE_BIN`, when set, is the only
/// profile used. Returns the first accepted output, or an error: a Cloudflare
/// challenge from any profile, otherwise the last error.
///
/// `args` must not set a User-Agent. Each profile sends its own browser's,
/// matching its TLS fingerprint and client hints; another one makes the
/// request look forged. On 2026-11-10 IDX passed `curl_chrome131` 5 times out
/// of 5 with its own User-Agent and once out of 5 with a fixed Chrome/122 one.
pub fn run_owned_with_fallback(
    stage: &str,
    args: &[String],
    accept: &dyn Fn(&[u8]) -> Result<(), IdxError>,
) -> Result<Output, IdxError> {
    debug_assert!(
        !args.iter().any(|arg| sets_user_agent(arg)),
        "{stage}: curl-impersonate profiles send their own User-Agent"
    );
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

    run_profiles(stage, &profiles, args, accept)
}

fn sets_user_agent(arg: &str) -> bool {
    arg.trim_start()
        .get(..11)
        .is_some_and(|name| name.eq_ignore_ascii_case("user-agent:"))
}

/// A challenge can differ by profile, so one round tries them all, but a repeat
/// straight away is unlikely to change the answer: it ends after that round.
fn run_profiles(
    stage: &str,
    profiles: &[String],
    args: &[String],
    accept: &dyn Fn(&[u8]) -> Result<(), IdxError>,
) -> Result<Output, IdxError> {
    let mut probed_args = vec!["--write-out".to_string(), PROBE_WRITE_OUT.to_string()];
    probed_args.extend_from_slice(args);

    let mut last_error = None;
    let mut challenge = None;
    for _ in 0..FALLBACK_ROUNDS {
        for binary in profiles {
            match run_binary(stage, binary, &probed_args) {
                Ok(output) => match accept(&output.stdout) {
                    Ok(()) => {
                        crate::runtime::verbose(format!("{stage}: fetched with {binary}"));
                        return Ok(output);
                    }
                    Err(err) => last_error = Some(err),
                },
                Err(err @ IdxError::CloudflareChallenge(_)) => challenge = Some(err),
                Err(err) => last_error = Some(err),
            }
        }
        if let Some(err) = challenge {
            return Err(err);
        }
    }
    Err(last_error.expect("at least one profile was tried"))
}

/// Every curl run gives up after this long, so a stalled server cannot hang
/// a command or an MCP call.
const MAX_TIME_SECS: &str = "600";
/// Largest response kept in memory (IDX report files are a few MB).
const MAX_RESPONSE_BYTES: usize = 256 * 1024 * 1024;

fn run_binary(stage: &str, binary: &str, args: &[String]) -> Result<Output, IdxError> {
    run_binary_limited(stage, binary, args, MAX_RESPONSE_BYTES)
}

/// `--max-filesize` cannot stop a response without a length, nor (before
/// curl 8.20) a small compressed one that decodes to more, so stdout is
/// also capped while it is read: past `limit`, curl is killed and reaped.
fn run_binary_limited(
    stage: &str,
    binary: &str,
    args: &[String],
    limit: usize,
) -> Result<Output, IdxError> {
    let max_filesize = limit.to_string();
    let mut child = Command::new(binary)
        .args(["--max-time", MAX_TIME_SECS, "--max-filesize", max_filesize.as_str()])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            return IdxError::Http(format!(
                "curl-impersonate binary '{binary}' not found; set {OVERRIDE_ENV} or install nixpkgs#curl-impersonate-chrome"
            ));
        }
        IdxError::Http(format!("failed to run {binary} for {stage}: {e}"))
    })?;

    // stderr drains on its own thread so a chatty curl cannot block on it.
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut stderr = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut stderr);
        stderr
    });
    let mut stdout = Vec::new();
    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let read = (&mut stdout_pipe)
        .take(limit as u64 + 1)
        .read_to_end(&mut stdout);
    if read.is_err() || stdout.len() > limit {
        // The curl_* profiles are scripts that run curl without exec, so the
        // kill reaches only the wrapper; curl itself still holds stderr. Do
        // not wait for it: closing stdout ends it on its next write (or
        // --max-time ends a stalled one), and the stderr thread exits with it.
        drop(stdout_pipe);
        let _ = child.kill();
        let _ = child.wait();
        drop(stderr_reader);
        return Err(match read {
            Err(e) => IdxError::Http(format!(
                "failed to read {stage} response from {binary}: {e}"
            )),
            Ok(_) => IdxError::Http(format!("{stage} response exceeds {limit} bytes")),
        });
    }
    let status = child
        .wait()
        .map_err(|e| IdxError::Http(format!("failed to wait for {binary} for {stage}: {e}")))?;
    let stderr = stderr_reader.join().unwrap_or_default();
    let (probe, stderr) = split_probe(&stderr);
    let output = Output {
        status,
        stdout,
        stderr,
    };

    if let Some(probe) = probe.filter(|probe| probe.challenged) {
        return Err(IdxError::CloudflareChallenge(format!(
            "{stage}: Cloudflare answered {binary} with a challenge (HTTP {}, cf-mitigated: challenge), which curl cannot solve; retry later",
            probe.status
        )));
    }

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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_user_agent_header() {
        assert!(sets_user_agent("User-Agent: Mozilla/5.0"));
        assert!(sets_user_agent("user-agent:x"));
        assert!(!sets_user_agent("Accept: application/json"));
        assert!(!sets_user_agent("--header"));
        assert!(!sets_user_agent("https://www.idx.co.id/user-agent:"));
    }

    #[test]
    fn output_past_the_limit_fails_at_once_behind_a_profile_wrapper() {
        // Like the curl_* profile scripts, the wrapper runs "curl" without
        // exec; this one writes too much, then stalls holding stderr open.
        let dir = std::env::temp_dir().join(format!("idx-curl-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("curl");
        std::fs::write(
            &fake,
            "#!/bin/sh\n( head -c 8192 /dev/zero; sleep 30 )\ntrue\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&fake, perms).unwrap();

        let started = std::time::Instant::now();
        let err = run_binary_limited("test", fake.to_str().unwrap(), &[], 4096).unwrap_err();

        assert!(err.to_string().contains("exceeds 4096 bytes"), "{err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
