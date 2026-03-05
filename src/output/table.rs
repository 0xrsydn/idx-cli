use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use owo_colors::OwoColorize;

use crate::api::types::{Ohlc, Quote};
use crate::error::IdxError;

pub fn format_idr(value: f64) -> String {
    let rounded = value.round() as i64;
    let chars: Vec<char> = rounded.to_string().chars().rev().collect();
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out.chars().rev().collect()
}

pub fn print_quotes(quotes: &[Quote], no_color: bool) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["SYMBOL", "PRICE", "CHG", "CHG%", "VOLUME", "MKT CAP"]);

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
            Cell::new(format!("{:+.2}", q.change)),
            pct_cell,
            Cell::new(format_idr(q.volume as f64)),
            Cell::new(
                q.market_cap
                    .map(format_idr)
                    .unwrap_or_else(|| "-".to_string()),
            ),
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
            Cell::new(format_idr(item.volume as f64)),
        ]);
    }
    println!("{table}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_idr;

    #[test]
    fn formats_idr_numbers() {
        assert_eq!(format_idr(9875.0), "9,875");
        assert_eq!(format_idr(1_215_200_000_000_000.0), "1,215,200,000,000,000");
    }
}
