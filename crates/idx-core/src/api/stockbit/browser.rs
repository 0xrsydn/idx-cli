//! Signing in to Stockbit inside a real browser, with no DevTools and no
//! copying.
//!
//! Stockbit publishes no OAuth client a third party can register, and its own
//! sign-in is a first-party flow guarded by reCAPTCHA, so a CLI cannot ask for
//! a token directly. What it can do is hand the sign-in to the browser that
//! already knows how to do it, then read the cookie that browser ends up with:
//!
//! 1. Launch a Chromium-family browser on Stockbit's sign-in page with a
//!    profile directory only idx uses and `--remote-debugging-port=0`. Chrome
//!    writes the port it picked and the WebSocket path to
//!    `<profile>/DevToolsActivePort`.
//! 2. Connect to that endpoint over WebSocket and ask for the cookies.
//! 3. Keep asking until the `credentialStorage` cookie holds tokens `verify`
//!    accepts, take it out of the profile, then close the browser.
//!
//! What Stockbit's web app does with that cookie (read from its own bundle,
//! 2026-10-29): it is the app's persisted credential store, written by
//! script, host-only on `stockbit.com`, for 30 days, as percent-encoded
//! `{"state":{"access","refresh","user"},"version":0}`. Every way in (password,
//! Google or Apple, a new device's one-time code, account recovery) and the
//! app's own `POST /login/refresh` end in that one store, so the cookie is
//! the single place a finished sign-in shows up. A signed-out page writes the
//! store too, without tokens, which is why a cookie counts only once
//! `auth::session_from_paste` finds tokens in it.
//!
//! The password, the one-time code and any reCAPTCHA stay in the browser and
//! on Stockbit's page: idx never sees them. The cookie travels in memory only,
//! and only as far as `verify`, which stores the two tokens.
//!
//! A dedicated profile is required by the browser (Chrome 136 and later refuse
//! remote debugging on the default profile) and wanted anyway: idx's session
//! never mixes with the user's everyday browsing.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};

use crate::error::IdxError;

/// The cookie Stockbit keeps its session in.
const COOKIE: &str = "credentialStorage";
/// The page the browser opens, which is Stockbit's own sign-in.
const SIGN_IN_URL: &str = "https://stockbit.com/login";
/// Profile directory idx keeps its own browser session in, beside
/// `config.toml`.
pub const PROFILE_DIR: &str = "stockbit-browser";
/// How long the browser may take to publish its DevTools endpoint.
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// Pause between cookie reads.
const POLL: Duration = Duration::from_millis(500);
/// How long a session check that failed for a passing reason (network, rate
/// limit) waits before the same cookie is checked again.
#[cfg(not(test))]
const VERIFY_RETRY: Duration = Duration::from_secs(5);
#[cfg(test)]
const VERIFY_RETRY: Duration = Duration::from_millis(50);
/// Socket timeout for one DevTools message.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the browser gets to quit after `Browser.close`.
const EXIT_GRACE: Duration = Duration::from_secs(3);
/// Largest DevTools message accepted.
const MAX_MESSAGE: u64 = 8 * 1024 * 1024;

/// Names the browser idx starts, ahead of any it finds.
pub const ENV_BROWSER: &str = "IDX_STOCKBIT_BROWSER";
/// File in the profile naming the executable that last opened it. A profile
/// belongs to one browser: its cookies are encrypted with that browser's own
/// key, and an older Chromium may refuse a profile a newer one wrote.
const MADE_BY: &str = "idx-browser";

/// A Chromium-family browser idx can drive over the DevTools protocol.
pub struct Browser {
    /// What to call the browser in messages (`Google Chrome`).
    pub name: String,
    /// Its executable.
    pub path: PathBuf,
}

impl Browser {
    /// The browser to open `profile` with: the one `IDX_STOCKBIT_BROWSER`
    /// names, else the one that opened that profile before, else the first
    /// supported browser installed here.
    ///
    /// Under the mock provider only the variable counts, so a test never
    /// opens a browser it did not ask for.
    pub fn find(profile: &Path) -> Result<Self, IdxError> {
        if let Some(path) = std::env::var_os(ENV_BROWSER) {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(IdxError::ConfigError(format!(
                    "{ENV_BROWSER} is {}, which is not a file",
                    path.display()
                )));
            }
            return Ok(Self::at(path));
        }
        #[cfg(feature = "testing")]
        if crate::api::mock::enabled() {
            return Err(IdxError::Unsupported(format!(
                "no browser under the mock provider; set {ENV_BROWSER}"
            )));
        }
        if let Some(browser) = Self::recorded(profile) {
            return Ok(browser);
        }
        candidates()
            .into_iter()
            .find(|(_, path)| path.is_file())
            .map(|(name, path)| Self {
                name: name.to_string(),
                path,
            })
            .ok_or_else(|| {
                IdxError::Unsupported(format!(
                    "no Chrome, Chromium, Edge, Brave or Helium found; set {ENV_BROWSER} to a \
                     Chromium-family browser"
                ))
            })
    }

    /// A browser known only by its executable.
    fn at(path: PathBuf) -> Self {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "the browser".to_string());
        Self { name, path }
    }

    /// The browser that last opened `profile`, if it is still installed.
    fn recorded(profile: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(profile.join(MADE_BY)).ok()?;
        let path = PathBuf::from(raw.trim());
        path.is_file().then(|| Self::at(path))
    }

    /// Opens the browser on Stockbit's sign-in page and connects to it.
    ///
    /// Returns once the browser answers over DevTools, so a browser that will
    /// not start is reported here, before anyone waits on it.
    pub fn open(&self, profile: &Path) -> Result<Window, IdxError> {
        let launched = self.launch(profile)?;
        let devtools = connect(profile, &|| launched.is_running())?;
        Ok(Window { launched, devtools })
    }

    /// Starts the browser on its sign-in page.
    fn launch(&self, profile: &Path) -> Result<Launched, IdxError> {
        std::fs::create_dir_all(profile)
            .map_err(|e| IdxError::Io(format!("cannot create idx's browser profile: {e}")))?;
        owner_only(profile);
        // A port file left by an earlier run would be read as this run's.
        let _ = std::fs::remove_file(profile.join("DevToolsActivePort"));
        let child = Command::new(&self.path)
            .args(self.args(profile))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| IdxError::Io(format!("cannot open {}: {e}", self.name)))?;
        // Best effort: without it, the next run searches again.
        if let Some(path) = self.path.to_str() {
            let _ = std::fs::write(profile.join(MADE_BY), path);
        }
        Ok(Launched {
            child: std::cell::RefCell::new(child),
        })
    }

    /// Arguments that keep idx's sign-in in its own profile and publish a
    /// DevTools port instead of a fixed one.
    fn args(&self, profile: &Path) -> Vec<String> {
        vec![
            format!("--user-data-dir={}", profile.display()),
            "--remote-debugging-port=0".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-search-engine-choice-screen".to_string(),
            SIGN_IN_URL.to_string(),
        ]
    }
}

/// A browser window idx opened on Stockbit's sign-in page.
pub struct Window {
    launched: Launched,
    devtools: DevTools,
}

impl Window {
    /// Waits for a session `verify` accepts, then closes the browser.
    ///
    /// A session already in idx's profile that still verifies is taken at
    /// once. One `verify` refuses is reported once and then left alone:
    /// signing in again in the open window replaces it. Setting `cancel`
    /// ends the wait and closes the browser too.
    pub fn wait(
        mut self,
        timeout: Duration,
        cancel: &AtomicBool,
        verify: &dyn Fn(&str) -> Result<(), IdxError>,
    ) -> Result<(), IdxError> {
        let launched = &self.launched;
        let result = wait_for_session(&mut self.devtools, timeout, verify, &|| {
            launched.is_running() && !cancel.load(Ordering::Relaxed)
        });
        self.launched.exit(EXIT_GRACE);
        result
    }
}

/// Connects to the browser that is starting on `profile`.
fn connect(profile: &Path, alive: &dyn Fn() -> bool) -> Result<DevTools, IdxError> {
    let (port, path) = read_endpoint(profile, START_TIMEOUT, alive)?;
    DevTools::connect(port, &path)
}

/// Waits on a running browser for a session `verify` accepts.
///
/// Apart from `Window` so tests can drive the wait against a stand-in
/// DevTools endpoint without launching a browser.
fn wait_for_session(
    devtools: &mut DevTools,
    timeout: Duration,
    verify: &dyn Fn(&str) -> Result<(), IdxError>,
    alive: &dyn Fn() -> bool,
) -> Result<(), IdxError> {
    let deadline = Instant::now() + timeout;
    let mut rejected: Option<String> = None;
    // After a check fails for a passing reason, the next is held back.
    let mut retry_at: Option<Instant> = None;
    let mut last_error: Option<String> = None;
    // The window is watched so closing it ends the wait instead of running
    // out the clock; the first polls may not see a page yet.
    let mut saw_window = false;
    let mut misses = 0;
    loop {
        match devtools.session_cookie() {
            Ok(Some(cookie))
                if rejected.as_deref() != Some(cookie.value.as_str())
                    && retry_at.is_none_or(|at| Instant::now() >= at) =>
            {
                match verify(&cookie.value) {
                    Ok(()) => {
                        // The session now lives in idx's own file. Take the
                        // copy out of the browser, but leave Stockbit's
                        // device cookies in place.
                        if let Err(err) = devtools.forget_session(&cookie) {
                            crate::runtime::warn(format!(
                                "could not remove the session cookie from idx's browser \
                                 profile ({err}); deleting that profile folder removes it"
                            ));
                        }
                        devtools.close();
                        return Ok(());
                    }
                    Err(err) if super::check_is_transient(&err) => {
                        crate::runtime::warn(format!(
                            "could not check the session in idx's browser yet ({err}); \
                             retrying"
                        ));
                        retry_at = Some(Instant::now() + VERIFY_RETRY);
                    }
                    Err(err) => {
                        crate::runtime::warn(format!(
                            "Stockbit refused the session in idx's browser ({err}); sign in \
                             again in that window"
                        ));
                        rejected = Some(cookie.value);
                        retry_at = None;
                    }
                }
            }
            // A later read that works clears an earlier failure, so the
            // timeout message names only what is still wrong.
            Ok(_) => last_error = None,
            Err(err) => last_error = Some(err.to_string()),
        }
        if !alive() {
            devtools.close();
            return Err(IdxError::Io(
                "the browser exited before idx got a session; run `idx stockbit login` again"
                    .to_string(),
            ));
        }
        match devtools.has_page() {
            Ok(true) => {
                saw_window = true;
                misses = 0;
            }
            Ok(false) => {
                misses += 1;
                if saw_window && misses >= 2 {
                    devtools.close();
                    return Err(IdxError::Io(
                        "the Stockbit sign-in window was closed before the session arrived; \
                         run `idx stockbit login` again"
                            .to_string(),
                    ));
                }
            }
            Err(err) => last_error = Some(err.to_string()),
        }
        if Instant::now() >= deadline {
            devtools.close();
            let mut message = format!(
                "the Stockbit sign-in timed out after {}s and idx closed its browser window; \
                 re-run `idx stockbit login`, or add --no-browser for the local page",
                timeout.as_secs()
            );
            if let Some(err) = last_error {
                message.push_str(&format!(" (last DevTools error: {err})"));
            }
            return Err(IdxError::Io(message));
        }
        std::thread::sleep(POLL);
    }
}

/// Where idx keeps its own browser profile.
pub fn profile_dir() -> Result<PathBuf, IdxError> {
    Ok(super::auth::session_dir()?.join(PROFILE_DIR))
}

/// Browsers to try, in order, for this platform.
fn candidates() -> Vec<(&'static str, PathBuf)> {
    let mut found = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let homes: Vec<PathBuf> = ["/Applications", "/System/Applications"]
            .iter()
            .map(PathBuf::from)
            .chain(std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Applications")))
            .collect();
        let apps = [
            (
                "Google Chrome",
                "Google Chrome.app/Contents/MacOS/Google Chrome",
            ),
            ("Chromium", "Chromium.app/Contents/MacOS/Chromium"),
            (
                "Microsoft Edge",
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            ),
            ("Brave", "Brave Browser.app/Contents/MacOS/Brave Browser"),
            ("Helium", "Helium.app/Contents/MacOS/Helium"),
            (
                "Google Chrome Canary",
                "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            ),
        ];
        for home in homes {
            for (name, rel) in apps {
                found.push((name, home.join(rel)));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let roots: Vec<PathBuf> = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        for root in roots {
            for (name, rel) in [
                ("Google Chrome", r"Google\Chrome\Application\chrome.exe"),
                ("Chromium", r"Chromium\Application\chrome.exe"),
                ("Microsoft Edge", r"Microsoft\Edge\Application\msedge.exe"),
                (
                    "Brave",
                    r"BraveSoftware\Brave-Browser\Application\brave.exe",
                ),
            ] {
                found.push((name, root.join(rel)));
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let names = [
            ("Google Chrome", "google-chrome"),
            ("Google Chrome", "google-chrome-stable"),
            ("Chromium", "chromium"),
            ("Chromium", "chromium-browser"),
            ("Microsoft Edge", "microsoft-edge"),
            ("Brave", "brave-browser"),
        ];
        let dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        for (name, exe) in names {
            for dir in &dirs {
                found.push((name, dir.join(exe)));
            }
        }
    }
    found
}

/// A browser process idx started and must not leave behind.
struct Launched {
    // A `RefCell` because the wait needs liveness through an immutable
    // borrow; the process is only touched from one thread.
    child: std::cell::RefCell<Child>,
}

impl Launched {
    /// Whether the browser is still running.
    fn is_running(&self) -> bool {
        matches!(self.child.borrow_mut().try_wait(), Ok(None))
    }

    /// Gives the browser `grace` to quit after `Browser.close`, then kills it.
    fn exit(&self, grace: Duration) {
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if !self.is_running() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        let child = self.child.get_mut();
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Reads `<profile>/DevToolsActivePort`: the port Chrome picked, then the
/// WebSocket path to the browser endpoint.
///
/// A browser that exits before writing it has usually handed the window to
/// one already running on the same profile, which idx cannot drive.
fn read_endpoint(
    profile: &Path,
    timeout: Duration,
    alive: &dyn Fn() -> bool,
) -> Result<(u16, String), IdxError> {
    let file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(raw) = std::fs::read_to_string(&file)
            && let Some((port, path)) = parse_endpoint(&raw)
        {
            return Ok((port, path));
        }
        if !alive() {
            return Err(IdxError::Io(
                "the browser exited as it started; if a window for idx is already open, close \
                 it and try again"
                    .to_string(),
            ));
        }
        if Instant::now() >= deadline {
            return Err(IdxError::Io(format!(
                "the browser did not publish a DevTools port within {}s; if a window for idx is \
                 already open, close it and try again",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The two lines of a `DevToolsActivePort` file.
fn parse_endpoint(raw: &str) -> Option<(u16, String)> {
    let mut lines = raw.lines();
    let port = lines.next()?.trim().parse::<u16>().ok()?;
    let path = lines.next()?.trim();
    (port != 0 && path.starts_with('/')).then(|| (port, path.to_string()))
}

/// Stockbit's session cookie as the browser holds it: the value, and the
/// domain and path that name it for deletion.
struct SessionCookie {
    value: String,
    domain: String,
    path: String,
}

/// The session cookie among DevTools' cookies, if the browser has one with
/// tokens in it; the store a signed-out page writes does not count.
fn session_cookie(cookies: &Value) -> Option<SessionCookie> {
    cookies
        .as_array()?
        .iter()
        .filter(|cookie| cookie["name"] == COOKIE)
        .filter_map(|cookie| {
            let domain = cookie["domain"].as_str()?;
            is_stockbit(domain).then(|| SessionCookie {
                value: cookie["value"].as_str().unwrap_or_default().to_string(),
                domain: domain.to_string(),
                path: cookie["path"].as_str().unwrap_or("/").to_string(),
            })
        })
        .find(|cookie| super::auth::session_from_paste(&cookie.value).is_some())
}

/// `stockbit.com` or a subdomain, as a cookie's domain names it (host-only
/// cookies have no leading dot).
fn is_stockbit(domain: &str) -> bool {
    let host = domain.strip_prefix('.').unwrap_or(domain);
    host == "stockbit.com" || host.ends_with(".stockbit.com")
}

/// A DevTools protocol connection: one WebSocket, one request at a time.
struct DevTools {
    stream: TcpStream,
    next_id: u64,
}

impl DevTools {
    fn connect(port: u16, path: &str) -> Result<Self, IdxError> {
        let io = |e: std::io::Error| IdxError::Io(format!("browser DevTools: {e}"));
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).map_err(io)?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(io)?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
        let key = base64::engine::general_purpose::STANDARD.encode(random_bytes());
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).map_err(io)?;
        let head = read_head(&mut stream)?;
        if !head.starts_with("HTTP/1.1 101") {
            return Err(IdxError::Io(format!(
                "browser DevTools refused the connection: {}",
                head.lines().next().unwrap_or_default()
            )));
        }
        Ok(Self { stream, next_id: 0 })
    }

    /// The `credentialStorage` cookie the browser holds, if it has tokens.
    fn session_cookie(&mut self) -> Result<Option<SessionCookie>, IdxError> {
        let result = self.call("Storage.getCookies", json!({}))?;
        Ok(session_cookie(&result["cookies"]))
    }

    /// Asks the browser to quit; it usually closes the socket instead of
    /// answering, which is not an error here.
    fn close(&mut self) {
        let _ = self
            .stream
            .set_read_timeout(Some(Duration::from_millis(500)));
        let _ = self.call("Browser.close", json!({}));
    }

    /// Whether the browser still has a window open.
    fn has_page(&mut self) -> Result<bool, IdxError> {
        Ok(!self.pages()?.is_empty())
    }

    /// Removes the session cookie from the profile, keeping the rest, and
    /// checks it is gone.
    ///
    /// Stockbit's script holds the session in memory, writes it back to the
    /// cookie, and syncs it across its tabs, so no Stockbit page may be open
    /// when the cookie goes: idx opens a blank tab of its own, closes every
    /// other one, and deletes from there. The cookie is named by the domain
    /// and path the browser reported, since Stockbit sets it host-only.
    fn forget_session(&mut self, cookie: &SessionCookie) -> Result<(), IdxError> {
        let created = self.call("Target.createTarget", json!({ "url": "about:blank" }))?;
        let blank = created["targetId"]
            .as_str()
            .ok_or_else(|| IdxError::Io("browser DevTools opened no tab".to_string()))?
            .to_string();
        for target in self.pages()? {
            if target != blank {
                self.call("Target.closeTarget", json!({ "targetId": target }))?;
            }
        }
        let attached = self.call(
            "Target.attachToTarget",
            json!({ "targetId": blank, "flatten": true }),
        )?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| IdxError::Io("browser DevTools attached no session".to_string()))?
            .to_string();
        self.call_in(
            Some(&session),
            "Network.deleteCookies",
            json!({ "name": COOKIE, "domain": cookie.domain, "path": cookie.path }),
        )?;
        if self.session_cookie()?.is_some() {
            return Err(IdxError::Io(
                "the session cookie was still there after deleting it".to_string(),
            ));
        }
        Ok(())
    }

    /// The ids of the browser's open tabs.
    fn pages(&mut self) -> Result<Vec<String>, IdxError> {
        let targets = self.call("Target.getTargets", json!({}))?;
        Ok(targets["targetInfos"]
            .as_array()
            .map(|infos| {
                infos
                    .iter()
                    .filter(|target| target["type"] == "page")
                    .filter_map(|target| target["targetId"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// One request and its reply, skipping events.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, IdxError> {
        self.call_in(None, method, params)
    }

    /// A request in a session (a page attached with `flatten`), or in the
    /// browser itself when `session` is `None`.
    fn call_in(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, IdxError> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let io = |e: std::io::Error| IdxError::Io(format!("browser DevTools: {e}"));
        self.stream
            .write_all(&frame(1, message.to_string().as_bytes()))
            .map_err(io)?;
        loop {
            let payload = self.read_message()?;
            let reply: Value = serde_json::from_slice(&payload)
                .map_err(|e| IdxError::Io(format!("browser DevTools: {e}")))?;
            if reply["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = reply.get("error") {
                return Err(IdxError::Io(format!(
                    "browser DevTools {method}: {}",
                    error["message"].as_str().unwrap_or("failed")
                )));
            }
            return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// One whole message, joining fragments and answering pings.
    fn read_message(&mut self) -> Result<Vec<u8>, IdxError> {
        let io = |e: std::io::Error| IdxError::Io(format!("browser DevTools: {e}"));
        let mut buffer = Vec::new();
        loop {
            let (fin, opcode, payload) = read_frame(&mut self.stream)?;
            match opcode {
                // ping: answer so the browser keeps the socket open.
                0x9 => self.stream.write_all(&frame(0xA, &payload)).map_err(io)?,
                // pong: nothing to do.
                0xA => continue,
                0x8 => {
                    return Err(IdxError::Io(
                        "browser DevTools closed the connection".to_string(),
                    ));
                }
                // text, binary, or a continuation of the message in hand.
                0x0 => buffer.extend_from_slice(&payload),
                0x1 | 0x2 => {
                    buffer.clear();
                    buffer.extend_from_slice(&payload);
                }
                _ => continue,
            }
            if fin {
                return Ok(buffer);
            }
        }
    }
}

/// Reads the HTTP response head, up to the empty line.
fn read_head(stream: &mut TcpStream) -> Result<String, IdxError> {
    let io = |e: std::io::Error| IdxError::Io(format!("browser DevTools: {e}"));
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).map_err(io)? == 0 {
            return Err(IdxError::Io(
                "browser DevTools closed the connection".to_string(),
            ));
        }
        raw.push(byte[0]);
        if raw.len() > 8192 {
            return Err(IdxError::Io(
                "browser DevTools sent an oversized handshake".to_string(),
            ));
        }
    }
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

/// A masked client frame: clients must mask, servers must not.
fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);
    let mask = 0x80;
    match payload.len() {
        len if len < 126 => out.push(mask | len as u8),
        len if len <= u16::MAX as usize => {
            out.push(mask | 126);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            out.push(mask | 127);
            out.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    let key = fastrand::u32(..).to_be_bytes();
    out.extend_from_slice(&key);
    out.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ key[index % 4]),
    );
    out
}

/// One frame: `(fin, opcode, payload)`.
fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), IdxError> {
    let io = |e: std::io::Error| IdxError::Io(format!("browser DevTools: {e}"));
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).map_err(io)?;
    let fin = head[0] & 0x80 != 0;
    let opcode = head[0] & 0x0F;
    let masked = head[1] & 0x80 != 0;
    let length = match head[1] & 0x7F {
        126 => {
            let mut bytes = [0u8; 2];
            reader.read_exact(&mut bytes).map_err(io)?;
            u16::from_be_bytes(bytes) as u64
        }
        127 => {
            let mut bytes = [0u8; 8];
            reader.read_exact(&mut bytes).map_err(io)?;
            u64::from_be_bytes(bytes)
        }
        short => u64::from(short),
    };
    if length > MAX_MESSAGE {
        return Err(IdxError::Io(format!(
            "browser DevTools sent an oversized message ({length} bytes)"
        )));
    }
    let key = if masked {
        let mut key = [0u8; 4];
        reader.read_exact(&mut key).map_err(io)?;
        Some(key)
    } else {
        None
    };
    let mut payload = vec![0u8; length as usize];
    reader.read_exact(&mut payload).map_err(io)?;
    if let Some(key) = key {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[index % 4];
        }
    }
    Ok((fin, opcode, payload))
}

fn random_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(4) {
        chunk.copy_from_slice(&fastrand::u32(..).to_be_bytes());
    }
    bytes
}

/// Owner-only permissions on the profile, which holds Stockbit's cookie.
fn owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::net::TcpListener;

    use super::*;

    /// A masked client frame round-trips through the reader.
    #[test]
    fn frames_round_trip() {
        for payload in [b"".as_slice(), b"hi", &[b'x'; 300], &[b'y'; 70_000]] {
            let raw = frame(1, payload);
            assert_eq!(raw[1] & 0x80, 0x80, "client frames are masked");
            let (fin, opcode, decoded) = read_frame(&mut Cursor::new(raw)).unwrap();
            assert!(fin);
            assert_eq!(opcode, 1);
            assert_eq!(decoded, payload);
        }
    }

    /// The port file is two lines, and anything else is not an endpoint.
    #[test]
    fn endpoint_needs_a_port_and_a_path() {
        assert_eq!(
            parse_endpoint("9222\n/devtools/browser/abc\n"),
            Some((9222, "/devtools/browser/abc".to_string()))
        );
        assert_eq!(parse_endpoint("0\n/devtools/browser/abc"), None);
        assert_eq!(parse_endpoint("9222\n"), None);
        assert_eq!(parse_endpoint(""), None);
    }

    /// What a signed-out Stockbit page stores: the credential store with no
    /// tokens (constructed from the app's `partialize`, which drops unset
    /// fields).
    const SIGNED_OUT: &str = "%7B%22state%22%3A%7B%7D%2C%22version%22%3A0%7D";

    /// A `credentialStorage` value with tokens in it.
    fn session_value() -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/stockbit_exodus_credential_storage_cookie_constructed.txt",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
        .trim()
        .to_string()
    }

    /// Only Stockbit's own cookie counts, and only once it holds tokens.
    #[test]
    fn only_a_stockbit_cookie_with_tokens_counts() {
        let session = session_value();
        let cookies = json!([
            {"name": COOKIE, "domain": ".stockbit.example", "path": "/", "value": session},
            {"name": COOKIE, "domain": "notstockbit.com", "path": "/", "value": session},
            {"name": "other", "domain": "stockbit.com", "path": "/", "value": session},
            {"name": COOKIE, "domain": "stockbit.com", "path": "/", "value": SIGNED_OUT},
            {"name": COOKIE, "domain": "stockbit.com", "path": "/", "value": session},
        ]);
        let found = session_cookie(&cookies).expect("the session cookie");
        assert_eq!(found.value, session);
        assert_eq!(
            found.domain, "stockbit.com",
            "host-only, as Stockbit sets it"
        );
        assert_eq!(found.path, "/");

        for value in [SIGNED_OUT, ""] {
            let lone = json!([{"name": COOKIE, "domain": "stockbit.com", "value": value}]);
            assert!(
                session_cookie(&lone).is_none(),
                "{value:?} is not a session"
            );
        }
        assert!(session_cookie(&json!([])).is_none());
        assert!(is_stockbit(".stockbit.com") && is_stockbit("exodus.stockbit.com"));
    }

    /// The browser must not share the user's profile, and must pick its port.
    #[test]
    fn launch_keeps_its_own_profile_and_port() {
        let browser = Browser {
            name: "test".to_string(),
            path: PathBuf::from("/bin/true"),
        };
        let args = browser.args(Path::new("/tmp/idx-profile"));
        assert!(args.contains(&"--user-data-dir=/tmp/idx-profile".to_string()));
        assert!(args.contains(&"--remote-debugging-port=0".to_string()));
        assert_eq!(args.last().map(String::as_str), Some(SIGN_IN_URL));
    }

    /// A whole sign-in against a stand-in browser: the session is handed to
    /// `verify`, every Stockbit tab is closed before the cookie is deleted by
    /// its own (host-only) domain, the cookie is gone afterwards, and the
    /// browser is asked to close.
    #[test]
    fn a_sign_in_takes_the_cookie_and_removes_it_from_the_profile() {
        let profile = temp_dir("sign-in");
        let browser = FakeBrowser::start(Fake::default());
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let seen = std::cell::RefCell::new(Vec::new());
        wait_for_session(
            &mut devtools,
            Duration::from_secs(5),
            &|pasted| {
                seen.borrow_mut().push(pasted.to_string());
                Ok(())
            },
            &|| true,
        )
        .unwrap();
        assert_eq!(seen.borrow().as_slice(), [session_value()]);

        let log = browser.finish();
        let at = |name: &str| log.methods.iter().position(|method| method == name);
        assert!(
            at("Target.closeTarget") < at("Network.deleteCookies"),
            "Stockbit's tab closes before the cookie goes: {:?}",
            log.methods
        );
        assert_eq!(log.closed, ["T1"], "the sign-in tab, not idx's blank one");
        assert!(!log.cookie_left, "the session cookie is out of the profile");
        assert_eq!(
            log.methods.last().map(String::as_str),
            Some("Browser.close")
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A cookie that survives the delete is an error, not a silent success.
    #[test]
    fn a_cookie_that_survives_the_delete_is_reported() {
        let profile = temp_dir("sticky");
        let browser = FakeBrowser::start(Fake {
            sticky: true,
            ..Fake::default()
        });
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let cookie = devtools.session_cookie().unwrap().expect("a session");
        let err = devtools.forget_session(&cookie).unwrap_err();
        assert!(err.to_string().contains("still there"), "{err}");
        devtools.close();
        assert!(browser.finish().cookie_left);
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// The store a signed-out page writes is not offered to `verify`.
    #[test]
    fn a_signed_out_page_is_not_a_session() {
        let profile = temp_dir("signed-out");
        let browser = FakeBrowser::start(Fake {
            cookie: Some(SIGNED_OUT.to_string()),
            ..Fake::default()
        });
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let checks = std::cell::Cell::new(0);
        let err = wait_for_session(
            &mut devtools,
            Duration::from_millis(300),
            &|_| {
                checks.set(checks.get() + 1);
                Ok(())
            },
            &|| true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
        assert_eq!(checks.get(), 0);
        browser.finish();
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A session idx refuses is reported once, and the wait goes on.
    #[test]
    fn a_refused_cookie_is_reported_once_and_the_wait_continues() {
        let profile = temp_dir("refused");
        let browser = FakeBrowser::start(Fake::default());
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let checks = std::cell::Cell::new(0);
        let err = wait_for_session(
            &mut devtools,
            Duration::from_millis(300),
            &|_| {
                checks.set(checks.get() + 1);
                Err(IdxError::AuthError("expired".to_string()))
            },
            &|| true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
        assert_eq!(checks.get(), 1, "one refusal per distinct cookie");
        browser.finish();
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A check that fails for a passing reason (a timeout, a 429) is not a
    /// refusal: the same cookie is checked again and can still sign in.
    #[test]
    fn a_check_that_fails_in_passing_is_retried() {
        let profile = temp_dir("transient");
        let browser = FakeBrowser::start(Fake::default());
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let checks = std::cell::Cell::new(0);
        wait_for_session(
            &mut devtools,
            Duration::from_secs(5),
            &|_| {
                checks.set(checks.get() + 1);
                match checks.get() {
                    1 => Err(IdxError::RateLimited),
                    _ => Ok(()),
                }
            },
            &|| true,
        )
        .unwrap();
        assert_eq!(checks.get(), 2);
        browser.finish();
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// Closing the window ends the wait at once, not at the timeout.
    #[test]
    fn closing_the_window_ends_the_wait() {
        let profile = temp_dir("closed");
        let browser = FakeBrowser::start(Fake {
            cookie: None,
            pages_for: 1,
            ..Fake::default()
        });
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let started = Instant::now();
        let err = wait_for_session(&mut devtools, Duration::from_secs(30), &|_| Ok(()), &|| {
            true
        })
        .unwrap_err();
        assert!(err.to_string().contains("window was closed"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited {:?}",
            started.elapsed()
        );
        browser.finish();
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A browser that quits ends the wait too.
    #[test]
    fn a_browser_that_quits_ends_the_wait() {
        let profile = temp_dir("quit");
        let browser = FakeBrowser::start(Fake {
            cookie: None,
            ..Fake::default()
        });
        write_endpoint(&profile, browser.port);
        let mut devtools = connect(&profile, &|| true).unwrap();
        let err = wait_for_session(&mut devtools, Duration::from_secs(30), &|_| Ok(()), &|| {
            false
        })
        .unwrap_err();
        assert!(err.to_string().contains("browser exited"), "{err}");
        browser.finish();
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// Writes a stand-in browser executable into `dir`.
    #[cfg(unix)]
    fn fake_executable(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join("fake-browser.sh");
        std::fs::write(&script, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        script
    }

    /// Launch, endpoint, WebSocket, verify, close: one sign-in end to end,
    /// with a stand-in executable in place of the browser.
    #[cfg(unix)]
    #[test]
    fn a_sign_in_starts_the_browser_and_closes_it() {
        let profile = temp_dir("launch");
        let server = FakeBrowser::start(Fake::default());

        // Writes the port file where the browser would, then waits to be
        // closed.
        let path = fake_executable(
            &profile,
            &format!(
                "for arg in \"$@\"; do\n\
                 case \"$arg\" in --user-data-dir=*) dir=\"${{arg#--user-data-dir=}}\" ;; esac\n\
                 done\n\
                 printf '%s\\n%s\\n' {port} /devtools/browser/test > \"$dir/DevToolsActivePort\"\n\
                 sleep 30\n",
                port = server.port
            ),
        );
        let browser = Browser {
            name: "fake".to_string(),
            path,
        };
        let started = Instant::now();
        browser
            .open(&profile)
            .unwrap()
            .wait(Duration::from_secs(5), &AtomicBool::new(false), &|_| Ok(()))
            .unwrap();
        assert_eq!(
            server.finish().methods.last().map(String::as_str),
            Some("Browser.close")
        );
        // The stand-in sleeps for 30 s; the sign-in must not wait for it.
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the sign-in waited {:?}",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// Cancelling ends the wait and closes the browser, so a server that
    /// is exiting leaves no window behind.
    #[cfg(unix)]
    #[test]
    fn cancelling_closes_the_browser() {
        let profile = temp_dir("cancel");
        let server = FakeBrowser::start(Fake {
            cookie: None,
            ..Fake::default()
        });
        let path = fake_executable(
            &profile,
            &format!(
                "for arg in \"$@\"; do\n\
                 case \"$arg\" in --user-data-dir=*) dir=\"${{arg#--user-data-dir=}}\" ;; esac\n\
                 done\n\
                 printf '%s\\n%s\\n' {port} /devtools/browser/test > \"$dir/DevToolsActivePort\"\n\
                 sleep 30\n",
                port = server.port
            ),
        );
        let browser = Browser {
            name: "fake".to_string(),
            path,
        };
        let window = browser.open(&profile).unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let waiter = {
            let cancel = cancel.clone();
            std::thread::spawn(move || window.wait(Duration::from_secs(30), &cancel, &|_| Ok(())))
        };
        std::thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert!(waiter.join().unwrap().is_err());
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "cancelling took {:?}",
            started.elapsed()
        );
        assert_eq!(
            server.finish().methods.last().map(String::as_str),
            Some("Browser.close")
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// The browser that opened a profile is named in it, and is the one used
    /// again; one no longer installed is not.
    #[cfg(unix)]
    #[test]
    fn a_profile_remembers_its_browser() {
        let profile = temp_dir("made-by");
        let path = fake_executable(&profile, "exit 0\n");
        assert!(Browser::recorded(&profile).is_none());
        let browser = Browser {
            name: "Named".to_string(),
            path: path.clone(),
        };
        // The launch records it even when the browser then goes away.
        assert!(browser.open(&profile).is_err());
        let again = Browser::recorded(&profile).expect("the recorded browser");
        assert_eq!(again.path, path);
        assert_eq!(again.name, "fake-browser");

        std::fs::remove_file(&path).unwrap();
        assert!(Browser::recorded(&profile).is_none());
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// A browser that hands its window to one already running exits at
    /// once; that is reported then, not after the start timeout.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_at_launch_fails_fast() {
        let profile = temp_dir("handoff");
        let browser = Browser {
            name: "fake".to_string(),
            path: fake_executable(&profile, "exit 0\n"),
        };
        let started = Instant::now();
        let err = browser.open(&profile).err().expect("no window");
        assert!(err.to_string().contains("exited as it started"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// The real thing: launches the browser named by `IDX_STOCKBIT_BROWSER`
    /// and checks the wait ends on the timeout, not on a broken connection,
    /// which is what proves the browser answered `Storage.getCookies`.
    ///
    /// Ignored by default: it opens a browser window and loads Stockbit's
    /// page, so it needs a machine with a browser and a network.
    #[test]
    #[ignore = "opens a real browser; set IDX_STOCKBIT_BROWSER"]
    fn a_real_browser_answers_the_devtools_requests() {
        let profile = temp_dir("real");
        let Ok(browser) = Browser::find(&profile) else {
            eprintln!("IDX_STOCKBIT_BROWSER is not set; nothing to launch");
            return;
        };
        let err = browser
            .open(&profile)
            .unwrap()
            .wait(Duration::from_secs(25), &AtomicBool::new(false), &|_| {
                panic!("a fresh profile holds no session to check")
            })
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("timed out"), "{message}");
        assert!(
            !message.contains("last DevTools error"),
            "DevTools never answered: {message}"
        );
        let _ = std::fs::remove_dir_all(&profile);
    }

    /// How a stand-in browser behaves.
    struct Fake {
        /// The session cookie's value, if it holds one.
        cookie: Option<String>,
        /// `Target.getTargets` reports pages for this many calls, then none.
        pages_for: usize,
        /// The cookie survives `Network.deleteCookies`.
        sticky: bool,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                cookie: Some(session_value()),
                pages_for: usize::MAX,
                sticky: false,
            }
        }
    }

    /// What a stand-in browser was asked, and what it was left holding.
    struct Log {
        methods: Vec<String>,
        /// Tabs closed with `Target.closeTarget`.
        closed: Vec<String>,
        cookie_left: bool,
    }

    /// A stand-in for a browser's DevTools endpoint: one WebSocket, scripted
    /// answers, and the methods it was asked for.
    struct FakeBrowser {
        port: u16,
        handle: Option<std::thread::JoinHandle<Log>>,
    }

    impl FakeBrowser {
        fn start(fake: Fake) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let handle = std::thread::spawn(move || serve(listener, fake));
            Self {
                port,
                handle: Some(handle),
            }
        }

        fn finish(mut self) -> Log {
            self.handle.take().unwrap().join().unwrap()
        }
    }

    /// Stockbit's cookie lives host-only on `stockbit.com`, and, as in
    /// Chromium, a delete must name that exact domain and path.
    fn serve(listener: TcpListener, fake: Fake) -> Log {
        const DOMAIN: &str = "stockbit.com";
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let head = read_head(&mut stream).unwrap();
        assert!(head.contains("Upgrade: websocket"), "{head}");
        stream
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\n\r\n")
            .unwrap();
        let mut cookie = fake.cookie.clone();
        let mut pages = vec!["T1".to_string()];
        let mut log = Log {
            methods: Vec::new(),
            closed: Vec::new(),
            cookie_left: false,
        };
        let mut target_calls = 0;
        while let Ok(payload) = read_message(&mut stream) {
            let request: Value = serde_json::from_slice(&payload).unwrap();
            let method = request["method"].as_str().unwrap_or_default().to_string();
            let params = &request["params"];
            log.methods.push(method.clone());
            if method == "Browser.close" {
                let reply = json!({ "id": request["id"], "result": {} });
                let _ = stream.write_all(&server_frame(reply.to_string().as_bytes()));
                break;
            }
            let result = match method.as_str() {
                "Storage.getCookies" => {
                    let cookies: Vec<Value> = cookie
                        .iter()
                        .map(|value| {
                            json!({ "name": COOKIE, "domain": DOMAIN, "path": "/", "value": value })
                        })
                        .collect();
                    json!({ "cookies": cookies })
                }
                "Target.getTargets" => {
                    target_calls += 1;
                    let infos: Vec<Value> = if target_calls <= fake.pages_for {
                        pages
                            .iter()
                            .map(|id| json!({ "targetId": id, "type": "page", "url": SIGN_IN_URL }))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    json!({ "targetInfos": infos })
                }
                "Target.createTarget" => {
                    pages.push("BLANK".to_string());
                    json!({ "targetId": "BLANK" })
                }
                "Target.closeTarget" => {
                    let id = params["targetId"].as_str().unwrap_or_default().to_string();
                    pages.retain(|page| *page != id);
                    log.closed.push(id);
                    json!({ "success": true })
                }
                "Target.attachToTarget" => json!({ "sessionId": "S1" }),
                "Network.deleteCookies" => {
                    assert_eq!(request["sessionId"], "S1", "deleted from a tab");
                    if !fake.sticky
                        && params["name"] == COOKIE
                        && params["domain"] == DOMAIN
                        && params["path"] == "/"
                    {
                        cookie = None;
                    }
                    json!({})
                }
                _ => json!({}),
            };
            let reply = json!({ "id": request["id"], "result": result });
            if stream
                .write_all(&server_frame(reply.to_string().as_bytes()))
                .is_err()
            {
                break;
            }
        }
        log.cookie_left = cookie.is_some();
        log
    }

    /// One unmasked server frame.
    fn server_frame(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x81];
        match payload.len() {
            len if len < 126 => out.push(len as u8),
            len => {
                out.push(126);
                out.extend_from_slice(&(len as u16).to_be_bytes());
            }
        }
        out.extend_from_slice(payload);
        out
    }

    fn read_message(stream: &mut TcpStream) -> Result<Vec<u8>, IdxError> {
        let mut buffer = Vec::new();
        loop {
            let (fin, _, payload) = read_frame(stream)?;
            buffer.extend_from_slice(&payload);
            if fin {
                return Ok(buffer);
            }
        }
    }

    fn write_endpoint(profile: &Path, port: u16) {
        std::fs::write(
            profile.join("DevToolsActivePort"),
            format!("{port}\n/devtools/browser/test\n"),
        )
        .unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "idx-browser-{name}-{}-{}",
            std::process::id(),
            fastrand::u32(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
