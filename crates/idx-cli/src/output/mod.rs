pub mod json;
pub mod style;
pub mod table;
pub mod text;

use std::io::IsTerminal;

use clap::ValueEnum;
use serde::Serialize;

use idx_core::analysis::fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport,
};
use idx_core::analysis::report::TechnicalReport;
use idx_core::api::types::{
    CompanyProfile, EarningsReport, FinancialStatements, InsightData, NewsItem, Ohlc, Quote,
    SentimentData,
};
use idx_core::broker::types::BrokerSummary;
use idx_core::error::IdxError;

/// Output format requested by the user (flag, `IDX_OUTPUT`, or config file).
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputSetting {
    // Table on a terminal, JSON when stdout is piped or captured.
    #[default]
    Auto,
    Table,
    Json,
    // JSON Lines: one array item per line.
    Jsonl,
}

impl OutputSetting {
    pub fn parse(value: &str) -> Option<Self> {
        <Self as ValueEnum>::from_str(value, true).ok()
    }

    pub fn resolve(self, stdout_is_terminal: bool) -> OutputFormat {
        match self {
            Self::Auto if stdout_is_terminal => OutputFormat::Table,
            Self::Auto | Self::Json => OutputFormat::Json,
            Self::Table => OutputFormat::Table,
            Self::Jsonl => OutputFormat::Jsonl,
        }
    }
}

/// Resolved output format used by renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Jsonl,
}

impl OutputFormat {
    /// True for every machine-readable format (JSON and JSON Lines).
    pub fn is_json(self) -> bool {
        matches!(self, Self::Json | Self::Jsonl)
    }
}

pub fn render_quotes(
    quotes: &[Quote],
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_quotes(quotes, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(quotes),
    }
}

pub fn render_broker_summary(
    summary: &BrokerSummary,
    format: &OutputFormat,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_broker_summary(summary),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(summary),
    }
}

pub fn render_history(
    symbol: &str,
    history: &[Ohlc],
    format: &OutputFormat,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_history(symbol, history),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(history),
    }
}

pub fn render_technical(
    report: &TechnicalReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_technical(report, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_growth(
    symbol: &str,
    report: &GrowthReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_growth(symbol, report, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_valuation(
    symbol: &str,
    report: &ValuationReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_valuation(symbol, report, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_risk(
    symbol: &str,
    report: &RiskReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_risk(symbol, report, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_fundamental(
    report: &FundamentalReport,
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_fundamental(report, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_compare(
    reports: &[FundamentalReport],
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_compare(reports, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(reports),
    }
}

pub fn render_profile(profile: &CompanyProfile, format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_profile(profile),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(profile),
    }
}

pub fn render_financials(
    financials: &FinancialStatements,
    format: &OutputFormat,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_financials(financials),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(financials),
    }
}

pub fn render_earnings(report: &EarningsReport, format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_earnings(report),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(report),
    }
}

pub fn render_sentiment(data: &SentimentData, format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_sentiment(data),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(data),
    }
}

pub fn render_insights(data: &InsightData, format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_insights(data),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(data),
    }
}

pub fn render_news(items: &[NewsItem], format: &OutputFormat) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_news(items),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(items),
    }
}

pub fn render_screener(
    quotes: &[Quote],
    format: &OutputFormat,
    no_color: bool,
) -> Result<(), IdxError> {
    match format {
        OutputFormat::Table => table::print_quotes(quotes, no_color),
        OutputFormat::Json | OutputFormat::Jsonl => json::print_json(quotes),
    }
}

pub fn emit_error(err: &IdxError, format: &OutputFormat) {
    if !format.is_json() {
        // Errors can carry provider, KSEI, or manifest text.
        eprintln!("Error: {}", text::terminal_safe(&err.to_string()));
        return;
    }
    let payload = serde_json::json!({
        "error": true,
        "code": err.code().to_string(),
        "message": err.to_string()
    });
    let pretty = *format == OutputFormat::Json && std::io::stderr().is_terminal();
    let rendered = if pretty {
        serde_json::to_string_pretty(&payload)
    } else {
        serde_json::to_string(&payload)
    };
    eprintln!("{}", rendered.unwrap_or_else(|_| "{}".to_string()));
}
