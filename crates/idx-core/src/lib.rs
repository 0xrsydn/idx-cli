//! idx-core: retrieval and analysis of Indonesian (IDX) market, financial, and
//! investor data, shared by the `idx` CLI and other frontends.
//! See docs/DESIGN.md.

pub mod analysis;
pub mod api;
pub mod cache;
mod client;
pub mod config;
pub mod curl_impersonate;
pub mod error;
pub mod financial;
pub mod market;
#[cfg(feature = "ownership")]
pub mod ownership;

pub use client::{CachePolicy, Fetched, Idx};

pub mod runtime {
    use std::fmt::Display;
    use std::sync::atomic::{AtomicBool, Ordering};

    static QUIET: AtomicBool = AtomicBool::new(false);

    pub fn set_quiet(quiet: bool) {
        QUIET.store(quiet, Ordering::Relaxed);
    }

    pub fn is_quiet() -> bool {
        QUIET.load(Ordering::Relaxed)
    }

    pub fn info(message: impl Display) {
        if !is_quiet() {
            eprintln!("info: {message}");
        }
    }

    pub fn warn(message: impl Display) {
        if !is_quiet() {
            eprintln!("warning: {message}");
        }
    }
}
