//! Terminal styling that turns itself off when color is disabled or stdout is
//! not a terminal, so piped output never carries ANSI escape codes.

use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};

use owo_colors::OwoColorize;

static COLOR: AtomicBool = AtomicBool::new(true);

pub fn set_color(enabled: bool) {
    COLOR.store(enabled, Ordering::Relaxed);
}

fn paint<T: Display>(value: T, styled: impl FnOnce(&T) -> String) -> String {
    if COLOR.load(Ordering::Relaxed) {
        styled(&value)
    } else {
        value.to_string()
    }
}

pub trait Paint: Display + Sized {
    fn bold(self) -> String {
        paint(self, |v| OwoColorize::bold(v).to_string())
    }
    fn green(self) -> String {
        paint(self, |v| OwoColorize::green(v).to_string())
    }
    fn red(self) -> String {
        paint(self, |v| OwoColorize::red(v).to_string())
    }
    fn yellow(self) -> String {
        paint(self, |v| OwoColorize::yellow(v).to_string())
    }
    // Only ownership output uses blue.
    #[cfg_attr(not(feature = "ownership"), allow(dead_code))]
    fn blue(self) -> String {
        paint(self, |v| OwoColorize::blue(v).to_string())
    }
}

impl<T: Display> Paint for T {}
