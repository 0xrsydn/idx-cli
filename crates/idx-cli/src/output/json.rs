use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;

use idx_core::error::IdxError;

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
            for line in json_lines(value).map_err(to_err)? {
                println!("{line}");
            }
        }
    }
    Ok(())
}

fn json_lines<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<String>> {
    let compact = serde_json::to_string(value)?;
    Ok(
        match serde_json::from_str::<Vec<&serde_json::value::RawValue>>(&compact) {
            Ok(items) => items.iter().map(|item| item.get().to_string()).collect(),
            Err(_) => vec![compact],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::json_lines;

    #[derive(serde::Serialize)]
    struct Row {
        symbol: &'static str,
        close: i64,
    }

    #[test]
    fn json_lines_keep_the_key_order_of_json_output() {
        let rows = [
            Row {
                symbol: "BBCA",
                close: 9000,
            },
            Row {
                symbol: "BBRI",
                close: 4000,
            },
        ];
        assert_eq!(
            json_lines(&rows).unwrap(),
            [
                r#"{"symbol":"BBCA","close":9000}"#,
                r#"{"symbol":"BBRI","close":4000}"#
            ]
        );
    }

    #[test]
    fn json_lines_print_a_non_array_as_one_line() {
        let row = Row {
            symbol: "BBCA",
            close: 9000,
        };
        assert_eq!(
            json_lines(&row).unwrap(),
            [r#"{"symbol":"BBCA","close":9000}"#]
        );
    }
}
