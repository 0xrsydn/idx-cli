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
        about = "Store a session pasted on stdin and check it with Stockbit",
        long_about = "Reads, on stdin, the value of stockbit.com's `credentialStorage` cookie \
(in a logged-in browser: DevTools, Application, Cookies, https://stockbit.com) or a refresh \
token, stores it, and checks it with one request. Servers can set \
IDX_STOCKBIT_REFRESH_TOKEN instead."
    )]
    Login,
    #[command(about = "Show whether a session is stored and when its tokens expire")]
    Status,
    #[command(about = "Delete the stored session")]
    Logout,
}

pub fn handle(cmd: &StockbitCmd, format: &OutputFormat) -> Result<(), IdxError> {
    match &cmd.command {
        StockbitSubcommand::Login => {
            let status = stockbit::login(&read_paste()?)?;
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
