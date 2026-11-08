//! `idx-mcp`: an MCP server (stdio) over idx-core. See docs/DESIGN.md.

mod params;
mod server;

use std::process::ExitCode;

use clap::Parser;
use idx_core::api::stockbit::signin;
use idx_core::config::CoreConfig;
use rmcp::ServiceExt;

#[derive(Debug, Parser)]
#[command(
    name = "idx-mcp",
    version,
    about = "MCP server (stdio) for Indonesian stock market (IDX) data"
)]
struct Args {
    /// Expose the ownership_sync tool, which downloads the KSEI ownership
    /// snapshot (several MB) into the local database.
    #[arg(long)]
    allow_sync: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let config = match CoreConfig::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(err.exit_code() as u8);
        }
    };

    let server = server::IdxServer::new(config, args.allow_sync);
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => running,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    tokio::select! {
        finished = running.waiting() => {
            let _ = tokio::task::spawn_blocking(signin::shutdown).await;
            match finished {
                Ok(_) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("error: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        () = terminated() => {
            let _ = tokio::task::spawn_blocking(signin::shutdown).await;
            // The stdin reader is a blocking thread the runtime would wait for.
            std::process::exit(0);
        }
    }
}

/// Resolves when the process is told to stop.
async fn terminated() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
