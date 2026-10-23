//! The user's own Stockbit session: two bearer JWTs, where they are kept, and
//! how they are refreshed. Opt-in; nothing here runs without a session.
//!
//! - The **access** token (about a day) authorises every
//!   `exodus.stockbit.com` request.
//! - The **refresh** token (about a week) is spent on `POST /login/refresh`,
//!   which answers a new access token *and a new refresh token*, retiring the
//!   one presented. So exactly one party may refresh a session: two idx
//!   processes refresh under a file lock and re-read what the other wrote, and
//!   a browser sharing the session gets logged out once idx refreshes.
//!
//! The session lives in `<config dir>/stockbit-session.json` (mode 0600).
//! `IDX_STOCKBIT_REFRESH_TOKEN` / `IDX_STOCKBIT_ACCESS_TOKEN` seed fields the
//! file lacks, for servers. Tokens never reach logs, errors or cache keys.
//! The token shapes follow fzn0x/idx (MIT).

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::IdxError;

pub const ENV_REFRESH: &str = "IDX_STOCKBIT_REFRESH_TOKEN";
pub const ENV_ACCESS: &str = "IDX_STOCKBIT_ACCESS_TOKEN";
const SESSION_FILE: &str = "stockbit-session.json";
const LOCK_FILE: &str = "stockbit-session.lock";
/// Refresh this long before the access token expires, so no request races
/// the expiry.
const REFRESH_BUFFER_SECS: i64 = 60 * 60;

pub(crate) const NO_SESSION: &str =
    "no Stockbit session; run `idx stockbit login` (or set IDX_STOCKBIT_REFRESH_TOKEN)";

/// The persisted token pair. Expiries are read from the tokens themselves.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Session {
    #[serde(default)]
    pub access: String,
    #[serde(default)]
    pub refresh: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("access", &(!self.access.is_empty()).then_some("<redacted>"))
            .field(
                "refresh",
                &(!self.refresh.is_empty()).then_some("<redacted>"),
            )
            .finish()
    }
}

impl Session {
    pub fn is_empty(&self) -> bool {
        self.access.is_empty() && self.refresh.is_empty()
    }
}

/// A token's presence and expiry, never its value.
#[derive(Debug, Clone, Serialize)]
pub struct TokenInfo {
    pub present: bool,
    /// When the token expires; `null` when absent or not a JWT.
    pub expires_at: Option<DateTime<Utc>>,
    /// Seconds until expiry, negative once expired.
    pub expires_in_secs: Option<i64>,
}

impl TokenInfo {
    fn of(token: &str, now: i64) -> Self {
        let present = !token.is_empty();
        let exp = present.then(|| token_expiry(token)).flatten();
        Self {
            present,
            expires_at: exp.and_then(|e| DateTime::from_timestamp(e, 0)),
            expires_in_secs: exp.map(|e| e - now),
        }
    }
}

/// What `idx stockbit status` and the MCP `stockbit_status` tool report.
#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    /// A refresh or access token is stored or seeded (live or not).
    pub logged_in: bool,
    pub access: TokenInfo,
    pub refresh: TokenInfo,
    pub session_file: String,
}

/// Reads, writes and refreshes the session.
#[derive(Debug, Clone)]
pub struct SessionStore {
    dir: PathBuf,
    seed: Session,
}

impl SessionStore {
    /// The session beside `config.toml`, seeded from the environment.
    pub fn open() -> Result<Self, IdxError> {
        let config = crate::config::config_path()?;
        let dir = config
            .parent()
            .ok_or_else(|| IdxError::ConfigError("config path has no directory".to_string()))?;
        let env = |name| {
            std::env::var(name)
                .map(|v| normalize_token(&v))
                .unwrap_or_default()
        };
        Ok(Self::at(
            dir.to_path_buf(),
            Session {
                access: env(ENV_ACCESS),
                refresh: env(ENV_REFRESH),
            },
        ))
    }

    pub fn at(dir: PathBuf, seed: Session) -> Self {
        Self { dir, seed }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(SESSION_FILE)
    }

    /// The file's tokens, each missing one taken from the seed.
    pub fn load(&self) -> Session {
        let mut session = std::fs::read_to_string(self.path())
            .ok()
            .and_then(|raw| serde_json::from_str::<Session>(&raw).ok())
            .unwrap_or_default();
        session.access = normalize_token(&session.access);
        session.refresh = normalize_token(&session.refresh);
        if session.access.is_empty() {
            session.access.clone_from(&self.seed.access);
        }
        if session.refresh.is_empty() {
            session.refresh.clone_from(&self.seed.refresh);
        }
        session
    }

    pub fn has_session(&self) -> bool {
        !self.load().is_empty()
    }

    /// Writes the session atomically, readable by the owner only.
    pub fn save(&self, session: &Session) -> Result<(), IdxError> {
        let io = |e: std::io::Error| IdxError::Io(format!("stockbit session: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let tmp = self.dir.join(format!("{SESSION_FILE}.tmp"));
        let body = serde_json::to_vec_pretty(session)
            .map_err(|e| IdxError::Io(format!("stockbit session: {e}")))?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp).map_err(io)?;
        std::io::Write::write_all(&mut file, &body).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        std::fs::rename(&tmp, self.path()).map_err(io)
    }

    /// Deletes the stored session; false when there was none. Seeds from the
    /// environment are not affected.
    pub fn clear(&self) -> Result<bool, IdxError> {
        match std::fs::remove_file(self.path()) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(IdxError::Io(format!("stockbit session: {e}"))),
        }
    }

    pub fn status(&self, now: i64) -> SessionStatus {
        let session = self.load();
        SessionStatus {
            logged_in: !session.is_empty(),
            access: TokenInfo::of(&session.access, now),
            refresh: TokenInfo::of(&session.refresh, now),
            session_file: self.path().display().to_string(),
        }
    }

    /// An access token good for at least `REFRESH_BUFFER_SECS`, refreshing
    /// with `refresher` (refresh token in, response body out) when it is not.
    pub fn access(
        &self,
        refresher: &dyn Fn(&str) -> Result<String, IdxError>,
        now: i64,
    ) -> Result<String, IdxError> {
        let current = self.load();
        if current.is_empty() {
            return Err(IdxError::AuthError(NO_SESSION.to_string()));
        }
        if fresh(&current.access, now) {
            return Ok(current.access);
        }
        self.locked(|| {
            // Another process may have refreshed while this one waited.
            let session = self.load();
            if fresh(&session.access, now) {
                return Ok(session.access);
            }
            match self.rotate(&session, refresher) {
                Ok(access) => Ok(access),
                // A failed refresh must not block a token that still works.
                Err(_) if token_expiry(&session.access).is_some_and(|exp| now < exp) => {
                    Ok(session.access)
                }
                Err(e) => Err(e),
            }
        })
    }

    /// A new access token after the server rejected `rejected`, unless
    /// another process already replaced it.
    pub fn after_rejection(
        &self,
        rejected: &str,
        refresher: &dyn Fn(&str) -> Result<String, IdxError>,
    ) -> Result<String, IdxError> {
        self.locked(|| {
            let session = self.load();
            if !session.access.is_empty() && session.access != rejected {
                return Ok(session.access);
            }
            self.rotate(&session, refresher)
        })
    }

    /// Spends the refresh token and stores the new pair.
    fn rotate(
        &self,
        session: &Session,
        refresher: &dyn Fn(&str) -> Result<String, IdxError>,
    ) -> Result<String, IdxError> {
        if session.refresh.is_empty() {
            return Err(IdxError::AuthError(
                "the Stockbit session has no refresh token; run `idx stockbit login`".to_string(),
            ));
        }
        let body = refresher(&session.refresh)?;
        let next = parse_refresh(&body, session)?;
        self.save(&next)?;
        Ok(next.access)
    }

    /// Runs `f` holding the session lock, so only one process spends the
    /// rotating refresh token at a time.
    fn locked<T>(&self, f: impl FnOnce() -> Result<T, IdxError>) -> Result<T, IdxError> {
        let io = |e: std::io::Error| IdxError::Io(format!("stockbit session lock: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let lock = lock_file(&self.dir.join(LOCK_FILE)).map_err(io)?;
        lock.lock().map_err(io)?;
        let result = f();
        drop(lock);
        result
    }
}

fn lock_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn fresh(access: &str, now: i64) -> bool {
    token_expiry(access).is_some_and(|exp| now < exp - REFRESH_BUFFER_SECS)
}

/// Strips whitespace and a leading `Bearer `.
pub fn normalize_token(raw: &str) -> String {
    let t = raw.trim();
    t.strip_prefix("Bearer ")
        .or_else(|| t.strip_prefix("bearer "))
        .unwrap_or(t)
        .trim()
        .to_string()
}

/// A JWT's `exp` (Unix seconds), read without verifying the signature: idx
/// only carries these tokens.
pub fn token_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_i64()
}

/// Three dot-separated base64url parts with a JSON header (`eyJ`).
fn looks_like_jwt(value: &str) -> bool {
    let mut parts = value.split('.');
    matches!(parts.next(), Some(h) if h.starts_with("eyJ"))
        && parts.next().is_some_and(|p| !p.is_empty())
        && parts.next().is_some_and(|p| !p.is_empty())
        && parts.next().is_none()
}

/// The JWT under key `slot` anywhere in `value`: a bare string or an object
/// with `token`. Stockbit's envelopes have moved between versions.
fn find_token(value: &Value, slot: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            let direct = map.get(slot).and_then(|hit| {
                hit.as_str()
                    .or_else(|| hit.get("token").and_then(Value::as_str))
                    .filter(|t| looks_like_jwt(t))
                    .map(str::to_string)
            });
            direct.or_else(|| map.values().find_map(|child| find_token(child, slot)))
        }
        Value::Array(items) => items.iter().find_map(|child| find_token(child, slot)),
        _ => None,
    }
}

/// The new pair from a `/login/refresh` response; a response that rotates
/// only the access token keeps the presented refresh token.
pub fn parse_refresh(body: &str, previous: &Session) -> Result<Session, IdxError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| IdxError::ParseError(format!("stockbit refresh response: {e}")))?;
    let access = find_token(&value, "access").ok_or_else(|| {
        IdxError::ParseError("stockbit refresh response carried no access token".to_string())
    })?;
    Ok(Session {
        access,
        refresh: find_token(&value, "refresh").unwrap_or_else(|| previous.refresh.clone()),
    })
}

/// A session from what a user pastes: the `credentialStorage` cookie of
/// stockbit.com (percent-encoded JSON), that JSON decoded, or a bare refresh
/// token.
pub fn session_from_paste(raw: &str) -> Option<Session> {
    let raw = raw.trim();
    let decoded = percent_decode(raw);
    for candidate in [decoded.as_str(), raw] {
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            let session = Session {
                access: find_token(&value, "access").unwrap_or_default(),
                refresh: find_token(&value, "refresh").unwrap_or_default(),
            };
            if !session.is_empty() {
                return Some(session);
            }
        }
    }
    let token = normalize_token(raw);
    looks_like_jwt(&token).then(|| Session {
        access: String::new(),
        refresh: token,
    })
}

/// `%XX` escapes decoded; anything else kept as is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// An unsigned JWT expiring at `exp`.
    fn jwt(exp: i64, tag: &str) -> String {
        let enc = |v: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.{}",
            enc(r#"{"alg":"none"}"#),
            enc(&format!(r#"{{"exp":{exp},"tag":"{tag}"}}"#)),
            enc("sig")
        )
    }

    fn refresh_body(access: &str, refresh: &str) -> String {
        format!(
            r#"{{"data":{{"access":{{"token":"{access}"}},"refresh":{{"token":"{refresh}"}}}}}}"#
        )
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("idx-stockbit-auth-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn reads_the_pair_from_a_refresh_response_and_the_cookie() {
        let pair = parse_refresh(
            &fixture("stockbit_exodus_login_refresh_constructed.json"),
            &Session::default(),
        )
        .unwrap();
        assert_eq!(token_expiry(&pair.access), Some(1_792_385_008));
        assert!(looks_like_jwt(&pair.refresh));

        let pasted = session_from_paste(&fixture(
            "stockbit_exodus_credential_storage_cookie_constructed.txt",
        ))
        .unwrap();
        assert!(looks_like_jwt(&pasted.access) && looks_like_jwt(&pasted.refresh));

        let bare = jwt(NOW, "r");
        assert_eq!(
            session_from_paste(&format!("  Bearer {bare}\n")),
            Some(Session {
                access: String::new(),
                refresh: bare
            })
        );
        assert_eq!(session_from_paste("not a token"), None);
    }

    #[test]
    fn a_refresh_without_a_new_refresh_token_keeps_the_old_one() {
        let previous = Session {
            access: String::new(),
            refresh: jwt(NOW, "old"),
        };
        let access = jwt(NOW + 86_400, "a");
        let next =
            parse_refresh(&format!(r#"{{"data":{{"access":"{access}"}}}}"#), &previous).unwrap();
        assert_eq!(
            next,
            Session {
                access,
                refresh: previous.refresh
            }
        );
    }

    #[test]
    fn a_fresh_access_token_is_used_without_refreshing() {
        let dir = TempDir::new("fresh");
        let store = SessionStore::at(dir.0.clone(), Session::default());
        let access = jwt(NOW + 2 * REFRESH_BUFFER_SECS, "a");
        store
            .save(&Session {
                access: access.clone(),
                refresh: jwt(NOW + 7 * 86_400, "r"),
            })
            .unwrap();
        let refresher = |_: &str| -> Result<String, IdxError> { panic!("must not refresh") };
        assert_eq!(store.access(&refresher, NOW).unwrap(), access);
    }

    #[test]
    fn a_stale_access_token_rotates_both_tokens_and_persists_them() {
        let dir = TempDir::new("rotate");
        let store = SessionStore::at(dir.0.clone(), Session::default());
        let old_refresh = jwt(NOW + 7 * 86_400, "r1");
        store
            .save(&Session {
                access: jwt(NOW + 60, "a1"),
                refresh: old_refresh.clone(),
            })
            .unwrap();
        let (new_access, new_refresh) = (jwt(NOW + 86_400, "a2"), jwt(NOW + 8 * 86_400, "r2"));
        let calls = Cell::new(0);
        let refresher = |presented: &str| {
            calls.set(calls.get() + 1);
            assert_eq!(presented, old_refresh);
            Ok(refresh_body(&new_access, &new_refresh))
        };
        assert_eq!(store.access(&refresher, NOW).unwrap(), new_access);
        assert_eq!(calls.get(), 1);
        // The rotated pair is what the next process reads.
        assert_eq!(
            store.load(),
            Session {
                access: new_access,
                refresh: new_refresh
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_rejected_token_already_replaced_by_another_process_is_not_refreshed_again() {
        let dir = TempDir::new("rejected");
        let store = SessionStore::at(dir.0.clone(), Session::default());
        let replaced = jwt(NOW + 86_400, "theirs");
        store
            .save(&Session {
                access: replaced.clone(),
                refresh: jwt(NOW + 86_400, "r"),
            })
            .unwrap();
        let refresher = |_: &str| -> Result<String, IdxError> { panic!("must not refresh") };
        assert_eq!(
            store
                .after_rejection(&jwt(NOW, "mine"), &refresher)
                .unwrap(),
            replaced
        );
    }

    #[test]
    fn no_session_is_an_auth_error_and_the_file_beats_the_seed() {
        let dir = TempDir::new("seed");
        let empty = SessionStore::at(dir.0.clone(), Session::default());
        let refresher = |_: &str| -> Result<String, IdxError> { unreachable!() };
        assert!(matches!(
            empty.access(&refresher, NOW),
            Err(IdxError::AuthError(_))
        ));

        let seeded = SessionStore::at(
            dir.0.clone(),
            Session {
                access: String::new(),
                refresh: jwt(NOW, "env"),
            },
        );
        assert!(seeded.has_session());
        let stored = jwt(NOW, "file");
        seeded
            .save(&Session {
                access: String::new(),
                refresh: stored.clone(),
            })
            .unwrap();
        assert_eq!(seeded.load().refresh, stored);
    }

    #[test]
    fn status_and_debug_never_show_a_token() {
        let dir = TempDir::new("status");
        let token = jwt(NOW + 100, "secret");
        let store = SessionStore::at(
            dir.0.clone(),
            Session {
                access: token.clone(),
                refresh: String::new(),
            },
        );
        let status = store.status(NOW);
        assert!(status.logged_in);
        assert_eq!(status.access.expires_in_secs, Some(100));
        assert!(!status.refresh.present);
        let shown = format!(
            "{} {:?}",
            serde_json::to_string(&status).unwrap(),
            store.load()
        );
        assert!(!shown.contains(&token), "{shown}");
    }
}
