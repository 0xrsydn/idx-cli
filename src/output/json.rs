use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;

use crate::error::IdxError;

const PRETTY: u8 = 0;
const COMPACT: u8 = 1;
const LINES: u8 = 2;

static STYLE: AtomicU8 = AtomicU8::new(PRETTY);

/// Pretty JSON only for a human at a terminal; compact for pipes and
/// captured output; JSON Lines when requested.
pub fn configure(format: super::OutputFormat, stdout_is_terminal: bool) {
    let style = match format {
        super::OutputFormat::Jsonl => LINES,
        _ if stdout_is_terminal => PRETTY,
        _ => COMPACT,
    };
    STYLE.store(style, Ordering::Relaxed);
}

pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<(), IdxError> {
    let to_err = |e: serde_json::Error| IdxError::ParseError(e.to_string());
    match STYLE.load(Ordering::Relaxed) {
        PRETTY => println!("{}", serde_json::to_string_pretty(value).map_err(to_err)?),
        COMPACT => println!("{}", serde_json::to_string(value).map_err(to_err)?),
        _ => {
            // Split an array into one line per item without re-serializing,
            // so each line keeps the same key order as `-o json`.
            let compact = serde_json::to_string(value).map_err(to_err)?;
            match serde_json::from_str::<Vec<&serde_json::value::RawValue>>(&compact) {
                Ok(items) => {
                    for item in items {
                        println!("{}", item.get());
                    }
                }
                Err(_) => println!("{compact}"),
            }
        }
    }
    Ok(())
}
