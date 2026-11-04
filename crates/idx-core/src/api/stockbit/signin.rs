//! One Stockbit sign-in for every frontend. `idx stockbit login` and the
//! `stockbit_login` MCP tool both start one here, and differ only in how
//! they wait: the CLI in the foreground, idx-mcp on a thread it polls.
//!
//! Two ways a session reaches idx, both ending in `accept`:
//! - **Browser** (`browser.rs`): idx opens a Chromium-family browser on
//!   Stockbit's own sign-in page, in a profile of its own, and reads the
//!   session cookie over DevTools. Nothing to copy; the default on a desktop.
//! - **Page** (`loopback.rs`): a page on 127.0.0.1 that takes the cookie from
//!   the user's own browser (bookmarklet, clipboard or paste). For a shell
//!   over SSH, a machine with no display, or no supported browser.
//!
//! `Method::Auto` picks the browser unless it would not reach the user, and
//! falls back to the page only when the browser cannot start. A sign-in that
//! started and then timed out or was closed is an error, not a fallback.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::auth::SessionStatus;
use super::{browser, loopback};
use crate::error::IdxError;

/// How long the browser window waits: the user signs in there, one-time code
/// and all.
pub const BROWSER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How long the page waits for a session from the user's browser.
pub const PAGE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Port the page prefers, so an `ssh -L` forward stays the same across runs.
pub const PAGE_PORT: u16 = 7878;
/// How long `shutdown` waits for a browser to close.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(6);

/// Which way to sign in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// The browser when it would reach the user, else the page.
    #[default]
    Auto,
    Browser,
    Page,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub method: Method,
    /// The page's port; `None` prefers `PAGE_PORT` and takes any free one
    /// when that is busy.
    pub port: Option<u16>,
    /// Open the page in the desktop's default browser, when this machine has
    /// a desktop the user is at.
    pub open_page: bool,
}

/// How a sign-in reaches the user.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Started {
    /// A stored session already works; nothing was opened.
    Existing,
    /// A browser window on Stockbit's sign-in page: sign in there.
    Browser { browser: String },
    /// The page on 127.0.0.1.
    Page {
        url: String,
        port: u16,
        /// The `ssh -L` command that brings the page to another machine.
        forward: String,
        /// The shell is over SSH, so the page is on another machine than the
        /// user's browser.
        remote: bool,
        /// Why the browser was not used, when `Auto` picked the page.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// A sign-in that has started and not yet been waited on.
pub struct SignIn {
    started: Started,
    timeout: Duration,
    waiting: Waiting,
}

enum Waiting {
    Browser(browser::Window),
    Page(loopback::LoginPage),
}

impl SignIn {
    /// Opens the browser window or the page, and returns once it is up.
    pub fn start(options: &Options) -> Result<Self, IdxError> {
        match options.method {
            Method::Browser => Self::browser(),
            Method::Page => Self::page(options, None),
            Method::Auto => match no_local_browser() {
                Some(reason) => Self::page(options, Some(reason.to_string())),
                None => Self::browser().or_else(|err| {
                    Self::page(
                        options,
                        Some(format!("the browser sign-in could not start: {err}")),
                    )
                }),
            },
        }
    }

    fn browser() -> Result<Self, IdxError> {
        let profile = browser::profile_dir()?;
        let browser = browser::Browser::find(&profile)?;
        let window = browser.open(&profile)?;
        Ok(Self {
            started: Started::Browser {
                browser: browser.name,
            },
            timeout: BROWSER_TIMEOUT,
            waiting: Waiting::Browser(window),
        })
    }

    fn page(options: &Options, reason: Option<String>) -> Result<Self, IdxError> {
        let page = match options.port {
            Some(port) => loopback::LoginPage::bind(port)?,
            None => {
                loopback::LoginPage::bind(PAGE_PORT).or_else(|_| loopback::LoginPage::bind(0))?
            }
        };
        if options.open_page && no_local_browser().is_none() {
            open_in_browser(&page.url());
        }
        Ok(Self {
            started: Started::Page {
                url: page.url(),
                port: page.port(),
                forward: loopback::ssh_forward(page.port()),
                remote: over_ssh(),
                reason,
            },
            timeout: PAGE_TIMEOUT,
            waiting: Waiting::Page(page),
        })
    }

    pub fn started(&self) -> &Started {
        &self.started
    }

    /// How long `wait` waits.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Waits for the user to finish, and reports the session stored.
    pub fn wait(self) -> Result<SessionStatus, IdxError> {
        self.wait_until(&AtomicBool::new(false))
    }

    /// `wait`, ending early with the browser closed when `cancel` is set.
    /// The page has nothing to close and stops with the process.
    fn wait_until(self, cancel: &AtomicBool) -> Result<SessionStatus, IdxError> {
        match self.waiting {
            Waiting::Browser(window) => window.wait(self.timeout, cancel, &accept)?,
            Waiting::Page(page) => page.wait(self.timeout, accept)?,
        }
        super::session_status()
    }
}

/// Checks a session value (the `credentialStorage` cookie or a refresh
/// token) and stores it: the step every way in ends with.
pub fn accept(value: &str) -> Result<(), IdxError> {
    super::login(value).map(|_| ())
}

/// Why a browser opened here would not reach the user, if it would not.
fn no_local_browser() -> Option<&'static str> {
    #[cfg(all(unix, not(target_os = "macos")))]
    let display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let display = true;
    blocker(over_ssh(), display)
}

fn blocker(ssh: bool, display: bool) -> Option<&'static str> {
    if ssh {
        Some("this shell is over SSH, so a browser started here opens on the remote machine")
    } else if !display {
        Some("this machine has no display (DISPLAY and WAYLAND_DISPLAY are unset)")
    } else {
        None
    }
}

fn over_ssh() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some()
}

/// Opens `url` with the desktop's default browser, best effort.
fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let (program, args) = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let (program, args) = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args) = ("xdg-open", vec![url]);
    let opened = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok();
    if !opened {
        crate::runtime::warn(format!("could not open a browser; open {url} yourself"));
    }
}

/// What a background sign-in is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Waiting,
    SignedIn,
    Failed,
}

/// A background sign-in as `start_background` and `progress` report it.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub state: State,
    #[serde(flatten)]
    pub started: Started,
    /// Seconds before a waiting sign-in gives up; 0 once it has ended.
    pub expires_in_secs: u64,
    /// True when `start_background` found this sign-in already waiting.
    pub already_open: bool,
    /// Why it failed: the error's code and message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProgressError>,
    /// The latest warning while it waits, such as Stockbit refusing the
    /// session signed in so far; gone once it signs in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressError {
    pub code: String,
    pub message: String,
}

struct Slot {
    id: u64,
    started: Started,
    opened: Instant,
    timeout: Duration,
    state: State,
    error: Option<ProgressError>,
    warning: Option<String>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Slot {
    fn progress(&self, already_open: bool) -> Progress {
        let expires_in_secs = match self.state {
            State::Waiting => self.timeout.saturating_sub(self.opened.elapsed()).as_secs(),
            _ => 0,
        };
        Progress {
            state: self.state,
            started: self.started.clone(),
            expires_in_secs,
            already_open,
            error: self.error.clone(),
            warning: self.warning.clone(),
        }
    }

    fn is_waiting(&self) -> bool {
        self.state == State::Waiting && self.opened.elapsed() < self.timeout
    }
}

/// This process's background sign-in, if one has run: one at a time.
static SLOT: Mutex<Option<Slot>> = Mutex::new(None);

/// Starts a sign-in on a thread and returns at once, for a frontend that
/// polls (`progress`) instead of blocking.
///
/// A stored session that still works means nothing to sign in to; a sign-in
/// already waiting is returned again rather than opening a second one.
pub fn start_background(options: &Options) -> Result<Progress, IdxError> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    // Held while the sign-in starts, so two calls never open two windows.
    let mut slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(open) = slot.as_ref().filter(|open| open.is_waiting()) {
        return Ok(open.progress(true));
    }
    if super::verify_session().is_ok() {
        return Ok(Progress {
            state: State::SignedIn,
            started: Started::Existing,
            expires_in_secs: 0,
            already_open: false,
            error: None,
            warning: None,
        });
    }
    let sign_in = SignIn::start(options)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut open = Slot {
        id,
        started: sign_in.started().clone(),
        opened: Instant::now(),
        timeout: sign_in.timeout(),
        state: State::Waiting,
        error: None,
        warning: None,
        cancel: cancel.clone(),
        thread: None,
    };
    let progress = open.progress(false);
    open.thread = Some(std::thread::spawn(move || {
        // Nobody reads a polling frontend's stderr, so what the wait warns
        // about (a refused session) goes into the progress it polls.
        let sink = crate::runtime::on_warning(move |message| {
            let mut slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(open) = slot.as_mut().filter(|open| open.id == id) {
                open.warning = Some(message.to_string());
            }
        });
        let result = sign_in.wait_until(&cancel);
        drop(sink);
        let mut slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(open) = slot.as_mut().filter(|open| open.id == id) {
            match result {
                Ok(_) => {
                    open.state = State::SignedIn;
                    open.warning = None;
                }
                Err(err) => {
                    open.state = State::Failed;
                    open.error = Some(ProgressError {
                        code: err.code().to_string(),
                        message: err.to_string(),
                    });
                }
            }
        }
    }));
    *slot = Some(open);
    Ok(progress)
}

/// Ends a background sign-in, for a process about to exit: a thread does not
/// outlive `main`, so without this its browser window would stay open with
/// nothing reading it.
pub fn shutdown() {
    let (thread, browser) = {
        let mut slot = SLOT.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(open) = slot.as_mut() else {
            return;
        };
        open.cancel.store(true, Ordering::Relaxed);
        (
            open.thread.take(),
            matches!(open.started, Started::Browser { .. }),
        )
    };
    let Some(thread) = thread.filter(|_| browser) else {
        return;
    };
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while !thread.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The background sign-in this process ran last, if any.
pub fn progress() -> Option<Progress> {
    SLOT.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|open| open.progress(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser is skipped exactly when it would not reach the user.
    #[test]
    fn the_browser_is_skipped_over_ssh_and_without_a_display() {
        assert_eq!(blocker(false, true), None);
        assert!(blocker(true, true).unwrap().contains("SSH"));
        assert!(blocker(false, false).unwrap().contains("no display"));
    }

    /// Frontends read the method from one field, whichever way was taken.
    #[test]
    fn progress_names_its_method_and_hides_what_it_does_not_have() {
        let page = Progress {
            state: State::Waiting,
            started: Started::Page {
                url: "http://127.0.0.1:7878/login/x".to_string(),
                port: 7878,
                forward: "ssh -L 7878:127.0.0.1:7878 me@host".to_string(),
                remote: false,
                reason: None,
            },
            expires_in_secs: 300,
            already_open: false,
            error: None,
            warning: None,
        };
        let value = serde_json::to_value(&page).unwrap();
        assert_eq!(value["state"], "waiting");
        assert_eq!(value["method"], "page");
        assert_eq!(value["port"], 7878);
        assert!(
            ["reason", "error", "warning"]
                .iter()
                .all(|key| value.get(key).is_none())
        );

        let failed = Progress {
            state: State::Failed,
            started: Started::Browser {
                browser: "Helium".to_string(),
            },
            expires_in_secs: 0,
            already_open: false,
            error: Some(ProgressError {
                code: "IO".to_string(),
                message: "the window was closed".to_string(),
            }),
            warning: Some("Stockbit refused the session".to_string()),
        };
        let value = serde_json::to_value(&failed).unwrap();
        assert_eq!(value["method"], "browser");
        assert_eq!(value["browser"], "Helium");
        assert_eq!(value["error"]["code"], "IO");
        assert_eq!(value["warning"], "Stockbit refused the session");
    }
}
