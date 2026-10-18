//! idx-core: retrieval and analysis of Indonesian (IDX) market, financial, and
//! investor data, shared by the `idx` CLI and other frontends.
//! See docs/DESIGN.md.

pub mod analysis;
pub mod api;
pub mod broker;
pub mod cache;
mod client;
pub mod config;
pub mod curl_impersonate;
pub mod error;
pub mod financial;
pub mod flow;
#[cfg(feature = "ownership")]
pub mod investor;
pub mod market;
#[cfg(feature = "ownership")]
pub mod ownership;
pub mod text;

pub use client::{CachePolicy, Fetched, Idx};

pub mod runtime {
    use std::fmt::Display;
    use std::sync::atomic::{AtomicBool, Ordering};

    static QUIET: AtomicBool = AtomicBool::new(false);
    static JSON: AtomicBool = AtomicBool::new(false);

    pub fn set_quiet(quiet: bool) {
        QUIET.store(quiet, Ordering::Relaxed);
    }

    pub fn is_quiet() -> bool {
        QUIET.load(Ordering::Relaxed)
    }

    /// In JSON mode diagnostics are JSON lines, so stderr stays
    /// machine-readable alongside a JSON error.
    pub fn set_json(json: bool) {
        JSON.store(json, Ordering::Relaxed);
    }

    fn emit(level: &str, message: impl Display) {
        if is_quiet() {
            return;
        }
        if JSON.load(Ordering::Relaxed) {
            let line = serde_json::json!({ "level": level, "message": message.to_string() });
            eprintln!("{line}");
        } else {
            eprintln!(
                "{level}: {}",
                crate::text::terminal_safe(&message.to_string())
            );
        }
    }

    pub fn info(message: impl Display) {
        emit("info", message);
    }

    pub fn warn(message: impl Display) {
        emit("warning", message);
    }
}
