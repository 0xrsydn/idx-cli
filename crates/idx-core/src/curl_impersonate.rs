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
    let mut command = Command::new(binary);
    command
        .args([
            "--max-time",
            MAX_TIME_SECS,
            "--max-filesize",
            max_filesize.as_str(),
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The curl_* profiles are scripts that run curl without exec; a process
    // group of its own lets an overflow kill curl too, not just the script.
    // Ctrl+C then reaches only idx: curl ends on its next write to the closed
    // pipe, or at --max-time.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
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
        drop(stdout_pipe);
        kill_process_group(&mut child);
        let _ = child.wait();
        let _ = stderr_reader.join();
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
    let output = Output {
        status,
        stdout,
        stderr,
    };

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

/// Kills curl and the profile script around it (their group, on Unix).
fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill(2) takes plain integers; the group is the one this
        // child leads (process_group(0) at spawn), not yet reaped.
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
        return;
    }
    let _ = child.kill();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_past_the_limit_fails_at_once_behind_a_profile_wrapper() {
        // Like the curl_* profile scripts, the wrapper runs "curl" without
        // exec; this one writes too much, then stalls holding stderr open.
        let dir = std::env::temp_dir().join(format!("idx-curl-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("curl");
        let pid_file = dir.join("curl.pid");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nsh -c 'echo $$ > {}; head -c 8192 /dev/zero; exec sleep 30'\ntrue\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&fake, perms).unwrap();

        let started = std::time::Instant::now();
        let err = run_binary_limited("test", fake.to_str().unwrap(), &[], 4096).unwrap_err();

        assert!(err.to_string().contains("exceeds 4096 bytes"), "{err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        // The stand-in curl behind the wrapper is gone too, not left to its
        // timeout (allow a moment for init to reap it).
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let alive = || unsafe { libc::kill(pid, 0) } == 0;
        for _ in 0..50 {
            if !alive() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!alive(), "curl {pid} outlived the overflow");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
