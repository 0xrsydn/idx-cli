pub mod json;
pub mod table;

use chrono::NaiveDate;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::analysis::signals::TechnicalSignal;
use crate::api::types::{Ohlc, Quote};
use crate::error::IdxError;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalReport {
    pub symbol: String,
    pub as_of: NaiveDate,
    pub current_price: i64,
    pub sma20: Option<f64>,
    pub sma50: Option<f64>,
    pub sma200: Option<f64>,
    pub rsi14: Option<f64>,
    pub macd: MacdSnapshot,
    pub volume: VolumeSnapshot,
    pub signals: TechnicalSignal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacdSnapshot {
    pub line: Option<f64>,
    pub signal: Option<f64>,
    pub histogram: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeSnapshot {
    pub current: u64,
    pub average20: Option<f64>,
    pub ratio20: Option<f64>,
}

pub fn render_quotes(
    quotes: &[Quote],
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_quotes(quotes, no_color),
        OutputFormat::Json => json::print_json(quotes),
    }
}

pub fn render_history(
    symbol: &str,
    history: &[Ohlc],
    format: &OutputFormat,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_history(symbol, history),
        OutputFormat::Json => json::print_json(history),
    }
}

pub fn render_technical(
    report: &TechnicalReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_technical(report, no_color),
        OutputFormat::Json => json::print_json(report),
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
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}
