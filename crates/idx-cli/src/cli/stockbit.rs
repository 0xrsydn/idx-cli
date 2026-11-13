use std::io::{IsTerminal, Read};

use clap::{Args, Subcommand};

use crate::output::text::terminal_safe;
use crate::output::{OutputFormat, json};
use idx_core::api::stockbit::{self, auth::SessionStatus, auth::TokenInfo, signin};
use idx_core::error::IdxError;
use idx_core::runtime;

#[derive(Debug, Args)]
#[command(
    about = "Your Stockbit session (opt-in: order book, live quotes)",
    long_about = "Your own Stockbit session, for `stocks orderbook`, live `--provider stockbit` \
quotes and Stockbit daily history. Opt-in and personal: idx uses your account's tokens, \
stored in the config directory with owner-only permissions, and never logs them. Stockbit \
rotates the refresh token on every refresh, so one session has one owner: `login` signs in \
through a browser profile only idx uses. Stockbit's terms allow personal, non-commercial use.",
    after_help = "Examples:\n  idx stockbit login                 # sign in through a browser\n  idx stockbit login --no-browser    # local page: bookmarklet or paste\n  pbpaste | idx stockbit login       # the credentialStorage cookie on stdin\n  idx stockbit status\n  idx stockbit logout"
)]
pub struct StockbitCmd {
    #[command(subcommand)]
    pub command: StockbitSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum StockbitSubcommand {
    #[command(
        about = "Sign in through your browser, or from stdin",
        long_about = "In a terminal, idx opens Chrome (or Chromium, Edge, Brave, Helium) on \
Stockbit's sign-in page with a profile only idx uses. Sign in there: idx reads the session \
cookie over the DevTools protocol, checks it with one request, takes it out of that profile, \
and closes the window. Your password, one-time codes and any reCAPTCHA stay in the browser. \
A stored session that still works means nothing to do: idx exits at once. Over SSH, without \
a display, without such a browser, or with --no-browser, idx serves a local page on \
127.0.0.1 instead, with a bookmarklet that reads the cookie in your own browser and a \
clipboard button. While the page waits you can also paste the cookie in the terminal, \
which needs no port forward when your browser is on another machine. With stdin piped, it reads the value of stockbit.com's \
`credentialStorage` cookie (DevTools, Application, Cookies) or a refresh token from stdin. \
Servers can set IDX_STOCKBIT_REFRESH_TOKEN instead; IDX_STOCKBIT_BROWSER picks the browser."
    )]
    Login {
        /// Use the local page instead of a browser sign-in
        #[arg(long)]
        no_browser: bool,
        /// Port for the local page (for `ssh -L`); default 7878, or any free one
        #[arg(long)]
        port: Option<u16>,
    },
    #[command(about = "Show whether a session is stored and when its tokens expire")]
    Status,
    #[command(about = "Delete the stored session")]
    Logout,
}

pub fn handle(cmd: &StockbitCmd, format: &OutputFormat) -> Result<(), IdxError> {
    match &cmd.command {
        StockbitSubcommand::Login { no_browser, port } => {
            let interactive = std::io::stdin().is_terminal();
            // A session that still works means there is nothing to sign in to.
            let reuse = interactive && stockbit::verify_session().is_ok();
            let status = if reuse {
                stockbit::session_status()?
            } else if interactive {
                sign_in(*no_browser, *port, format)?
            } else {
                stockbit::login(&read_paste()?)?
            };
            if format.is_json() {
                return json::print_json(&status);
            }
            if reuse {
                println!(
                    "Already signed in to Stockbit; use `idx stockbit logout` to switch accounts."
                );
            } else {
                println!("Stored and checked your Stockbit session.");
            }
            print_status(&status);
        }
        StockbitSubcommand::Status => {
            let status = stockbit::session_status()?;
            if format.is_json() {
                return json::print_json(&status);
            }
            print_status(&status);
        }
        StockbitSubcommand::Logout => {
            let (removed, status) = stockbit::logout()?;
            if status.logged_in {
                runtime::warn(
                    "IDX_STOCKBIT_REFRESH_TOKEN or IDX_STOCKBIT_ACCESS_TOKEN still provides a session",
                );
            }
            if format.is_json() {
                return json::print_json(&serde_json::json!({
                    "removed": removed,
                    "logged_in": status.logged_in,
                    "session_file": status.session_file,
                }));
            }
            if removed {
                println!("Removed the stored Stockbit session.");
            } else {
                println!("No Stockbit session was stored.");
            }
        }
    }
    Ok(())
}

/// Runs idx-core's sign-in in the foreground, telling the user on stderr
/// where to go. When neither the browser nor the page can start, the cookie
/// can still be pasted.
fn sign_in(
    no_browser: bool,
    port: Option<u16>,
    format: &OutputFormat,
) -> Result<SessionStatus, IdxError> {
    let options = signin::Options {
        method: if no_browser {
            signin::Method::Page
        } else {
            signin::Method::Auto
        },
        port,
        open_page: !no_browser,
    };
    let sign_in = match signin::SignIn::start(&options) {
        Ok(sign_in) => sign_in,
        Err(err) => {
            runtime::warn(format!("cannot start a sign-in ({err}); paste instead"));
            return stockbit::login(&read_paste()?);
        }
    };
    describe(sign_in.started(), format);
    if matches!(sign_in.started(), signin::Started::Page { .. }) {
        return sign_in.wait_with_pastes(&terminal_pastes());
    }
    sign_in.wait()
}

/// Longest line a Unix terminal hands over in one read; a longer paste
/// arrives cut.
const TERMINAL_LINE_LIMIT: usize = 4095;

/// Lines typed or pasted in the terminal while the page waits, so a user
/// whose browser cannot reach the page (a remote shell with no port forward)
/// still signs in from the same command. The thread ends with the process.
fn terminal_pastes() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if line.len() >= TERMINAL_LINE_LIMIT {
                runtime::warn(
                    "the terminal cut that paste short; pipe it instead \
                     (`pbpaste | ssh <host> idx stockbit login`) or paste only the refresh token",
                );
            } else if !line.trim().is_empty() && tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Where to sign in. These lines are the instructions, so they print even
/// with `--quiet`; on stderr, so stdout keeps only the result.
fn describe(started: &signin::Started, format: &OutputFormat) {
    let say = |message: String| {
        if format.is_json() {
            eprintln!(
                "{}",
                serde_json::json!({ "level": "info", "message": message })
            );
        } else {
            eprintln!("{}", terminal_safe(&message));
        }
    };
    match started {
        signin::Started::Browser { browser } => say(format!(
            "Opened {browser} on Stockbit's sign-in page. Sign in there; idx closes the window \
             when you are done."
        )),
        signin::Started::Page {
            url,
            forward,
            remote,
            reason,
            ..
        } => {
            if let Some(reason) = reason {
                runtime::warn(format!("{reason}; using the local page"));
            }
            say(format!("Open this page to connect Stockbit: {url}"));
            if *remote {
                say(format!(
                    "The page is on this machine (127.0.0.1). If your browser is on another \
                     machine, do one of these:\n  \
                     1. Forward the page: run this where the browser is, keep it open, then \
                     open the URL above there:\n       {forward}\n  \
                     2. Paste the session here: on stockbit.com, open DevTools > Application > \
                     Cookies, copy the value of `credentialStorage`, then paste it in this \
                     terminal and press Enter."
                ));
            } else {
                say(
                    "Or paste the value of stockbit.com's `credentialStorage` cookie in this \
                     terminal and press Enter."
                        .to_string(),
                );
            }
        }
        signin::Started::Existing => {}
    }
}

/// The pasted session: one line from a terminal, else all of stdin.
fn read_paste() -> Result<String, IdxError> {
    let stdin = std::io::stdin();
    let mut raw = String::new();
    if stdin.is_terminal() {
        eprintln!(
            "Paste the value of stockbit.com's `credentialStorage` cookie (DevTools > \
             Application > Cookies) or a refresh token, then press Enter:"
        );
        stdin
            .read_line(&mut raw)
            .map_err(|e| IdxError::Io(e.to_string()))?;
    } else {
        stdin
            .lock()
            .read_to_string(&mut raw)
            .map_err(|e| IdxError::Io(e.to_string()))?;
    }
    if raw.trim().is_empty() {
        return Err(IdxError::InvalidInput(
            "nothing on stdin: run `idx stockbit login` in a terminal to sign in through a \
             browser, pipe the credentialStorage cookie (`pbpaste | ssh <host> idx stockbit \
             login`), or set IDX_STOCKBIT_REFRESH_TOKEN"
                .to_string(),
        ));
    }
    Ok(raw)
}

fn print_status(status: &SessionStatus) {
    println!(
        "logged in:     {}",
        if status.logged_in { "yes" } else { "no" }
    );
    println!("access token:  {}", token_line(&status.access));
    println!("refresh token: {}", token_line(&status.refresh));
    println!("session file:  {}", terminal_safe(&status.session_file));
    if !status.logged_in {
        println!("Run `idx stockbit login` to add one.");
    }
}

fn token_line(token: &TokenInfo) -> String {
    match (token.present, token.expires_at, token.expires_in_secs) {
        (false, _, _) => "none".to_string(),
        (true, Some(at), Some(secs)) if secs > 0 => format!(
            "expires {} UTC (in {}h {}m)",
            at.format("%Y-%m-%d %H:%M"),
            secs / 3600,
            secs % 3600 / 60
        ),
        (true, Some(at), _) => format!("expired {} UTC", at.format("%Y-%m-%d %H:%M")),
        (true, None, _) => "present (expiry unknown)".to_string(),
    }
}
