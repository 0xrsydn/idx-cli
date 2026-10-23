use std::io::{IsTerminal, Read};

use clap::{Args, Subcommand};

use crate::output::text::terminal_safe;
use crate::output::{OutputFormat, json};
use idx_core::api::stockbit::{self, auth::SessionStatus, auth::TokenInfo};
use idx_core::error::IdxError;
use idx_core::runtime;

#[derive(Debug, Args)]
#[command(
    about = "Your Stockbit session (opt-in: order book, live quotes)",
    long_about = "Your own Stockbit session, for `stocks orderbook`, live `--provider stockbit` \
quotes and Stockbit daily history. Opt-in and personal: idx uses your account's tokens, \
stored in the config directory with owner-only permissions, and never logs them. Stockbit \
rotates the refresh token on every refresh, so a browser sharing the session is logged out \
once idx refreshes: log in through a separate browser profile for idx. Stockbit's terms \
allow personal, non-commercial use.",
    after_help = "Examples:\n  idx stockbit login                 # paste the credentialStorage cookie\n  pbpaste | idx stockbit login\n  idx stockbit status\n  idx stockbit logout"
)]
pub struct StockbitCmd {
    #[command(subcommand)]
    pub command: StockbitSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum StockbitSubcommand {
    #[command(
        about = "Connect a session in your browser, or from stdin",
        long_about = "In a terminal, opens a local page on 127.0.0.1 with two one-click ways to \
hand idx your Stockbit session: a bookmarklet that reads stockbit.com's cookie in your own \
browser, or a read-the-clipboard button. Without a browser (or with --no-browser), the page \
URL is printed instead. With stdin piped, it reads the value of stockbit.com's \
`credentialStorage` cookie (DevTools, Application, Cookies) or a refresh token from stdin, \
as before. Either way idx checks the session with one request. Servers can set \
IDX_STOCKBIT_REFRESH_TOKEN instead."
    )]
    Login {
        /// Print the page URL instead of opening a browser
        #[arg(long)]
        no_browser: bool,
        /// Fixed port for the local page (for `ssh -L`); default: any free one
        #[arg(long)]
        port: Option<u16>,
    },
    #[command(about = "Show whether a session is stored and when its tokens expire")]
    Status,
    #[command(about = "Delete the stored session")]
    Logout,
}

/// How long the local page waits for the session.
const LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub fn handle(cmd: &StockbitCmd, format: &OutputFormat) -> Result<(), IdxError> {
    match &cmd.command {
        StockbitSubcommand::Login { no_browser, port } => {
            let status = if std::io::stdin().is_terminal() {
                match login_in_browser(port.unwrap_or(0), !no_browser) {
                    Ok(()) => stockbit::session_status()?,
                    Err(err) => {
                        runtime::warn(format!("browser login unavailable ({err}); paste instead"));
                        stockbit::login(&read_paste()?)?
                    }
                }
            } else {
                stockbit::login(&read_paste()?)?
            };
            if format.is_json() {
                return json::print_json(&status);
            }
            println!("Stored and checked your Stockbit session.");
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

/// Serves the local login page and waits for a session it can verify.
fn login_in_browser(port: u16, open: bool) -> Result<(), IdxError> {
    let page = stockbit::loopback::LoginPage::bind(port)?;
    println!("Open this page to connect Stockbit: {}", page.url());
    if open {
        open_in_browser(&page.url());
    }
    page.wait(LOGIN_TIMEOUT, |pasted| {
        stockbit::login(pasted)
            .map(|_| ())
            .map_err(|err| err.to_string())
    })
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
        runtime::warn(format!("could not open a browser; open {url} yourself"));
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
            "nothing pasted: pipe or paste the credentialStorage cookie value or a refresh token"
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
