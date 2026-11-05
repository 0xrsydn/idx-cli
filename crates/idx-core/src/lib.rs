//! idx-core: retrieval and analysis of Indonesian (IDX) market, financial, and
//! investor data, shared by the `idx` CLI and other frontends.
//! See docs/DESIGN.md.

pub mod analysis;
pub mod api;
pub mod broker;
pub mod cache;
mod client;
pub mod config;
pub mod corporate;
pub mod curl_impersonate;
pub mod error;
pub mod financial;
pub mod flow;
#[cfg(feature = "ownership")]
pub mod investor;
pub mod market;
pub mod movements;
#[cfg(feature = "ownership")]
pub mod ownership;
pub mod text;

pub use client::{CachePolicy, Fetched, Idx};

pub mod runtime {
    use std::cell::RefCell;
    use std::fmt::Display;
    use std::sync::atomic::{AtomicBool, Ordering};

    static QUIET: AtomicBool = AtomicBool::new(false);
    static JSON: AtomicBool = AtomicBool::new(false);

    type Sink = Box<dyn Fn(&str)>;

    thread_local! {
        static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    }

    /// Sends this thread's warnings to `sink` too, until the guard drops: a
    /// frontend whose stderr nobody reads (idx-mcp) hands them on this way.
    /// Warnings still go to stderr, and `set_quiet` does not silence `sink`.
    pub fn on_warning(sink: impl Fn(&str) + 'static) -> WarningSink {
        let previous = SINK.with(|slot| slot.replace(Some(Box::new(sink))));
        WarningSink { previous }
    }

    /// Restores the thread's previous sink when dropped.
    #[must_use = "warnings reach the sink only while this guard lives"]
    pub struct WarningSink {
        previous: Option<Sink>,
    }

    impl Drop for WarningSink {
        fn drop(&mut self) {
            let previous = self.previous.take();
            SINK.with(|slot| *slot.borrow_mut() = previous);
        }
    }

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
        let message = message.to_string();
        SINK.with(|slot| {
            if let Some(sink) = slot.borrow().as_ref() {
                sink(&message);
            }
        });
        emit("warning", message);
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::rc::Rc;

        /// A sink sees this thread's warnings while its guard lives, and an
        /// inner one hands back to the outer when it drops.
        #[test]
        fn warnings_reach_the_sink_while_its_guard_lives() {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let outer = {
                let seen = seen.clone();
                on_warning(move |m| seen.borrow_mut().push(format!("outer {m}")))
            };
            warn("one");
            {
                let seen = seen.clone();
                let _inner = on_warning(move |m| seen.borrow_mut().push(format!("inner {m}")));
                warn("two");
            }
            warn("three");
            drop(outer);
            warn("four");
            info("not a warning");
            assert_eq!(*seen.borrow(), ["outer one", "inner two", "outer three"]);
        }
    }
}
