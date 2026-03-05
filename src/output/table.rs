use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use owo_colors::OwoColorize;

use crate::analysis::signals::Signal;
use crate::api::types::{Ohlc, Quote};
use crate::error::IdxError;
use crate::output::TechnicalReport;

pub fn format_idr(value: i64) -> String {
    let chars: Vec<char> = value.to_string().chars().rev().collect();
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out.chars().rev().collect()
}

pub fn format_u64(value: u64) -> String {
    format_idr(value as i64)
}

fn format_52w_range_bar(position: Option<f64>) -> String {
    let Some(pos) = position else {
        return "-".to_string();
    };

    let clamped = pos.clamp(0.0, 1.0);
    let filled = (clamped * 10.0).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
}

pub fn print_quotes(quotes: &[Quote], no_color: bool) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "SYMBOL",
            "PRICE",
            "CHG",
            "CHG%",
            "VOLUME",
            "MKT CAP",
            "52W RANGE",
        ]);

    for q in quotes {
        let pct = format!("{:+.2}%", q.change_pct);
        let pct_cell = if no_color {
            Cell::new(pct)
        } else if q.change_pct >= 0.0 {
            Cell::new(pct).fg(Color::Green)
        } else {
            Cell::new(pct).fg(Color::Red)
        };
        table.add_row(vec![
            Cell::new(&q.symbol),
            Cell::new(format_idr(q.price)),
            Cell::new(format!("{:+}", q.change)),
            pct_cell,
            Cell::new(format_u64(q.volume)),
            Cell::new(
                q.market_cap
                    .map(format_u64)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            Cell::new(format_52w_range_bar(q.week52_position)),
        ]);
    }

    println!("{table}");
    Ok(())
}

pub fn print_history(symbol: &str, history: &[Ohlc]) -> Result<(), IdxError> {
    println!("{}", format!("History for {symbol}").bold());
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["DATE", "OPEN", "HIGH", "LOW", "CLOSE", "VOLUME"]);
    for item in history {
        table.add_row(vec![
            Cell::new(item.date),
            Cell::new(format_idr(item.open)),
            Cell::new(format_idr(item.high)),
            Cell::new(format_idr(item.low)),
            Cell::new(format_idr(item.close)),
            Cell::new(format_u64(item.volume)),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn print_technical(report: &TechnicalReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!(
            "Technical Analysis for {} ({})",
            report.symbol, report.as_of
        )
        .bold()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        Cell::new("Current Price"),
        Cell::new(format_idr(report.current_price)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("SMA 20"),
        Cell::new(format_idr_option(report.sma20)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("SMA 50"),
        Cell::new(format_idr_option(report.sma50)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("SMA 200"),
        Cell::new(format_idr_option(report.sma200)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("RSI (14)"),
        Cell::new(format_float(report.rsi14, 2)),
        Cell::new(format_signal(report.signals.rsi, no_color, false)),
    ]);
    table.add_row(vec![
        Cell::new("MACD (12,26,9)"),
        Cell::new(format!(
            "{}/{}/{}",
            format_float(report.macd.line, 2),
            format_float(report.macd.signal, 2),
            format_float(report.macd.histogram, 2)
        )),
        Cell::new(format_signal(report.signals.macd, no_color, false)),
    ]);
    table.add_row(vec![
        Cell::new("Trend"),
        Cell::new(trend_context(report)),
        Cell::new(format_signal(report.signals.trend, no_color, false)),
    ]);
    table.add_row(vec![
        Cell::new("Volume Ratio (20)"),
        Cell::new(format_volume_ratio(report)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("Overall Signal"),
        Cell::new("-"),
        Cell::new(format_signal(report.signals.overall, no_color, true)),
    ]);

    println!("{table}");
    Ok(())
}

fn format_idr_option(value: Option<f64>) -> String {
    value
        .map(|v| format_idr(v.round() as i64))
        .unwrap_or_else(|| "-".to_string())
}

fn format_float(value: Option<f64>, precision: usize) -> String {
    value
        .map(|v| format!("{v:.prec$}", prec = precision))
        .unwrap_or_else(|| "-".to_string())
}

fn format_signal(signal: Signal, no_color: bool, uppercase: bool) -> String {
    let label = if uppercase {
        signal_label_upper(signal)
    } else {
        signal_label(signal)
    };

    if no_color {
        return label.to_string();
    }

    match signal {
        Signal::Bullish => label.green().to_string(),
        Signal::Bearish => label.red().to_string(),
        Signal::Neutral => label.yellow().to_string(),
    }
}

fn signal_label(signal: Signal) -> &'static str {
    match signal {
        Signal::Bullish => "Bullish",
        Signal::Bearish => "Bearish",
        Signal::Neutral => "Neutral",
    }
}

fn signal_label_upper(signal: Signal) -> &'static str {
    match signal {
        Signal::Bullish => "BULLISH",
        Signal::Bearish => "BEARISH",
        Signal::Neutral => "NEUTRAL",
    }
}

fn trend_context(report: &TechnicalReport) -> String {
    match (report.sma50, report.sma200) {
        (Some(sma50), Some(sma200)) => format!(
            "{} vs SMA50 {}, SMA200 {}",
            format_idr(report.current_price),
            format_idr(sma50.round() as i64),
            format_idr(sma200.round() as i64)
        ),
        _ => "Insufficient data".to_string(),
    }
}

fn format_volume_ratio(report: &TechnicalReport) -> String {
    match (report.volume.ratio20, report.volume.average20) {
        (Some(ratio), Some(avg)) => format!(
            "{ratio:.2}x ({} vs {} avg)",
            format_u64(report.volume.current),
            format_u64(avg.round() as u64)
        ),
        _ => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_idr, format_signal, format_u64};
    use crate::analysis::signals::Signal;

    #[test]
    fn formats_idr_numbers() {
        assert_eq!(format_idr(9875), "9,875");
        assert_eq!(format_u64(1_215_200_000_000_000), "1,215,200,000,000,000");
    }

    #[test]
    fn formats_plain_signal_labels() {
        assert_eq!(format_signal(Signal::Bullish, true, false), "Bullish");
        assert_eq!(format_signal(Signal::Bearish, true, true), "BEARISH");
    }
}
