pub mod json;
pub mod table;

use clap::ValueEnum;
use serde::Serialize;

use crate::api::types::{Ohlc, Quote};
use crate::error::IdxError;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

pub fn render_quotes(quotes: &[Quote], format: &OutputFormat, no_color: bool) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_quotes(quotes, no_color),
        OutputFormat::Json => json::print_json(quotes),
    }
}

pub fn render_history(symbol: &str, history: &[Ohlc], format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_history(symbol, history),
        OutputFormat::Json => json::print_json(history),
    }
}

pub fn emit_error(err: &IdxError, format: &OutputFormat) {
    match format {
        OutputFormat::Table => eprintln!("Error: {err}"),
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "error": true,
                "code": format!("{:?}", err.code()).to_uppercase(),
                "message": err.to_string()
            });
            eprintln!("{}", serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()));
        }
    }
}
