use crate::output::style::Paint;
use crate::output::text::{cell, painted, terminal_safe, truncate_chars};
use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};

use idx_core::analysis::fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport,
};
use idx_core::analysis::report::TechnicalReport;
use idx_core::analysis::signals::Signal;
use idx_core::api::types::{
    CompanyProfile, EarningsData, EarningsReport, FinancialStatements, InsightData, NewsItem, Ohlc,
    Quote, SentimentData,
};
use idx_core::error::IdxError;

pub fn format_idr(value: i64) -> String {
    let sign = if value.is_negative() { "-" } else { "" };
    let chars: Vec<char> = value.unsigned_abs().to_string().chars().rev().collect();
    let mut out = String::new();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    format!("{sign}{}", out.chars().rev().collect::<String>())
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
        let pct_cell = match q.change_pct {
            None => cell("-"),
            Some(pct) if no_color => cell(format!("{pct:+.2}%")),
            Some(pct) if pct >= 0.0 => cell(format!("{pct:+.2}%")).fg(Color::Green),
            Some(pct) => cell(format!("{pct:+.2}%")).fg(Color::Red),
        };
        table.add_row(vec![
            cell(&q.symbol),
            cell(format_idr(q.price)),
            cell(
                q.change
                    .map(|c| format!("{c:+}"))
                    .unwrap_or_else(|| "-".to_string()),
            ),
            pct_cell,
            cell(q.volume.map(format_u64).unwrap_or_else(|| "-".to_string())),
            cell(
                q.market_cap
                    .map(format_u64)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            cell(format_52w_range_bar(q.week52_position)),
        ]);
    }

    println!("{table}");
    for note in stale_quote_notes(quotes, chrono::Utc::now()) {
        println!("{}", terminal_safe(&note));
    }
    Ok(())
}

/// A quote whose last trade is over a week old is almost always a suspended
/// stock; say so under the table instead of letting it pass as current.
fn stale_quote_notes(quotes: &[Quote], now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    quotes
        .iter()
        .filter_map(|q| {
            let as_of = q.as_of?;
            (now.signed_duration_since(as_of) > chrono::Duration::days(7)).then(|| {
                format!(
                    "Note: {} last traded on {}; this price is not current.",
                    q.symbol,
                    as_of.date_naive()
                )
            })
        })
        .collect()
}

pub fn print_history(symbol: &str, history: &[Ohlc]) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("History for {}", terminal_safe(symbol)).bold()
    );
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["DATE", "OPEN", "HIGH", "LOW", "CLOSE", "VOLUME"]);
    for item in history {
        table.add_row(vec![
            cell(item.date),
            cell(format_idr(item.open)),
            cell(format_idr(item.high)),
            cell(format_idr(item.low)),
            cell(format_idr(item.close)),
            cell(format_u64(item.volume)),
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
        cell("Current Price"),
        cell(format_idr(report.current_price)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("SMA 20"),
        cell(format_idr_option(report.sma20)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("SMA 50"),
        cell(format_idr_option(report.sma50)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("SMA 200"),
        cell(format_idr_option(report.sma200)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("RSI (14)"),
        cell(format_float(report.rsi14, 2)),
        painted(format_signal(report.signals.rsi, no_color, false)),
    ]);
    table.add_row(vec![
        cell("MACD (12,26,9)"),
        cell(format!(
            "{}/{}/{}",
            format_float(report.macd.line, 2),
            format_float(report.macd.signal, 2),
            format_float(report.macd.histogram, 2)
        )),
        painted(format_signal(report.signals.macd, no_color, false)),
    ]);
    table.add_row(vec![
        cell("Trend"),
        cell(trend_context(report)),
        painted(format_signal(report.signals.trend, no_color, false)),
    ]);
    table.add_row(vec![
        cell("Volume Ratio (20)"),
        cell(format_volume_ratio(report)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("Overall Signal"),
        cell("-"),
        painted(format_signal(report.signals.overall, no_color, true)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_growth(symbol: &str, report: &GrowthReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("Growth Analysis for {}", terminal_safe(symbol)).bold()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        cell("Revenue Growth"),
        cell(format_pct(report.revenue_growth_pct)),
        painted(format_growth_signal(&report.revenue_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("Earnings Growth"),
        cell(format_pct(report.earnings_growth_pct)),
        painted(format_growth_signal(&report.earnings_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("Overall"),
        cell("-"),
        painted(format_growth_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_valuation(
    symbol: &str,
    report: &ValuationReport,
    no_color: bool,
) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("Valuation Analysis for {}", terminal_safe(symbol)).bold()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        cell("P/E (Trailing)"),
        cell(format_opt_f64(report.pe_trailing, 2)),
        painted(format_valuation_signal(&report.pe_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("P/E (Forward)"),
        cell(format_opt_f64(report.pe_forward, 2)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("Price/Book"),
        cell(format_opt_f64(report.pb, 2)),
        painted(format_valuation_signal(&report.pb_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("ROE"),
        cell(format_pct(report.roe_pct)),
        painted(format_valuation_signal(&report.roe_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("Net Margin"),
        cell(format_pct(report.net_margin_pct)),
        painted(format_valuation_signal(&report.margin_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("EV/EBITDA"),
        cell(format_opt_f64(report.ev_ebitda, 2)),
        painted(format_valuation_signal(&report.ev_ebitda_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("Overall"),
        cell("-"),
        painted(format_valuation_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_risk(symbol: &str, report: &RiskReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("Risk Analysis for {}", terminal_safe(symbol)).bold()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        cell("Debt/Equity"),
        cell(format_opt_f64(report.debt_to_equity, 2)),
        painted(format_risk_signal(&report.de_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("Current Ratio"),
        cell(format_opt_f64(report.current_ratio, 2)),
        painted(format_risk_signal(&report.current_ratio_signal, no_color)),
    ]);
    table.add_row(vec![
        cell("ROA"),
        cell(format_pct(report.roa_pct)),
        cell("-"),
    ]);
    table.add_row(vec![
        cell("Overall"),
        cell("-"),
        painted(format_risk_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_fundamental(report: &FundamentalReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("Fundamental Analysis for {}", terminal_safe(&report.symbol)).bold()
    );
    println!();
    print_growth(&report.symbol, &report.growth, no_color)?;
    println!();
    print_valuation(&report.symbol, &report.valuation, no_color)?;
    println!();
    print_risk(&report.symbol, &report.risk, no_color)?;
    println!();
    println!(
        "{} {}",
        "Overall Signal:".bold(),
        format_growth_signal(&report.overall_signal, no_color)
    );
    Ok(())
}

pub fn print_compare(reports: &[FundamentalReport], no_color: bool) -> Result<(), IdxError> {
    println!("{}", "Fundamental Comparison".bold());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    let mut header = vec![cell("METRIC")];
    header.extend(reports.iter().map(|report| cell(&report.symbol)));
    table.set_header(header);

    add_compare_row(
        &mut table,
        "Symbol",
        reports
            .iter()
            .map(|report| report.symbol.clone())
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "Overall",
        reports
            .iter()
            .map(|report| format_growth_signal(&report.overall_signal, no_color))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "Growth",
        reports
            .iter()
            .map(|report| format_growth_signal(&report.growth.overall_signal, no_color))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "Valuation",
        reports
            .iter()
            .map(|report| format_valuation_signal(&report.valuation.overall_signal, no_color))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "Risk",
        reports
            .iter()
            .map(|report| format_risk_signal(&report.risk.overall_signal, no_color))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "P/E",
        reports
            .iter()
            .map(|report| format_opt_f64(report.valuation.pe_trailing, 2))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "ROE",
        reports
            .iter()
            .map(|report| format_pct(report.valuation.roe_pct))
            .collect::<Vec<_>>(),
    );
    add_compare_row(
        &mut table,
        "Revenue Growth",
        reports
            .iter()
            .map(|report| format_pct(report.growth.revenue_growth_pct))
            .collect::<Vec<_>>(),
    );

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
        .map(|v| format!("{v:.precision$}"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_idr_option_from_f64(value: Option<f64>) -> String {
    value
        .map(|v| format_idr(v.round() as i64))
        .unwrap_or_else(|| "-".to_string())
}

fn format_table_date(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }

    trimmed.split('T').next().unwrap_or(trimmed).to_string()
}

fn format_table_date_option(value: Option<&str>) -> String {
    value
        .filter(|raw| !raw.trim().is_empty())
        .map(format_table_date)
        .unwrap_or_else(|| "-".to_string())
}

fn humanize_metric_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();

    for (idx, ch) in chars.iter().enumerate() {
        if matches!(ch, '_' | '-' | ' ') {
            if !current.is_empty() {
                words.push(current);
                current = String::new();
            }
            continue;
        }

        let next = chars.get(idx + 1).copied();
        let boundary = current.chars().last().is_some_and(|prev| {
            (prev.is_ascii_lowercase() && ch.is_ascii_uppercase())
                || (prev.is_ascii_alphabetic() && ch.is_ascii_digit())
                || (prev.is_ascii_digit() && ch.is_ascii_alphabetic())
                || (prev.is_ascii_uppercase()
                    && ch.is_ascii_uppercase()
                    && next.is_some_and(|next_ch| next_ch.is_ascii_lowercase()))
        });

        if boundary {
            words.push(current);
            current = String::new();
        }

        current.push(*ch);
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
        .iter()
        .map(|word| humanize_metric_word(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn humanize_metric_word(word: &str) -> String {
    let upper = word.to_ascii_uppercase();
    match upper.as_str() {
        "EPS" | "DPS" | "EBIT" | "EBITDA" | "GAAP" | "IDR" | "CIQ" => upper,
        _ if word.chars().all(|ch| ch.is_ascii_uppercase()) && word.len() <= 4 => word.to_string(),
        _ => {
            let mut chars = word.chars();
            let first = chars.next().unwrap_or_default().to_ascii_uppercase();
            format!("{first}{}", chars.as_str().to_ascii_lowercase())
        }
    }
}

fn format_earnings_period(period: &str) -> String {
    let trimmed = period.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }

    if trimmed.len() == 4 && trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return format!("FY{trimmed}");
    }

    // ASCII only, so the byte slices below fall on character boundaries.
    if trimmed.len() == 6
        && trimmed.is_ascii()
        && trimmed.starts_with('Q')
        && trimmed[1..].bytes().all(|byte| byte.is_ascii_digit())
    {
        return format!("{} {}", &trimmed[..2], &trimmed[2..]);
    }

    trimmed.to_string()
}

fn format_opt_f64(value: Option<f64>, precision: usize) -> String {
    format_float(value, precision)
}

fn format_pct(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:+.2}%"))
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
        _ => "Trend unavailable (need at least 200 daily candles)".to_string(),
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

fn format_growth_signal(signal: &str, no_color: bool) -> String {
    format_text_signal(
        signal,
        no_color,
        &["strong", "moderate", "growing", "healthy"],
        &["contracting", "declining", "shrinking", "weak"],
    )
}

fn format_valuation_signal(signal: &str, no_color: bool) -> String {
    format_text_signal(
        signal,
        no_color,
        &["deep value", "undervalued", "excellent", "strong"],
        &["expensive", "negative"],
    )
}

fn format_risk_signal(signal: &str, no_color: bool) -> String {
    format_text_signal(
        signal,
        no_color,
        &["conservative", "strong", "adequate", "low risk"],
        &["highly leveraged", "weak", "high risk", "negative equity"],
    )
}

fn format_text_signal(
    signal: &str,
    no_color: bool,
    positive: &[&str],
    negative: &[&str],
) -> String {
    // The label comes from the analysis report; make it safe before styling.
    let label = terminal_safe(signal);
    if no_color {
        return label;
    }

    if positive.contains(&signal) {
        label.green().to_string()
    } else if negative.contains(&signal) {
        label.red().to_string()
    } else {
        label.yellow().to_string()
    }
}

fn add_compare_row(table: &mut Table, label: &str, values: Vec<String>) {
    let mut row = vec![cell(label)];
    row.extend(values.into_iter().map(Cell::new));
    table.add_row(row);
}

pub fn print_profile(profile: &CompanyProfile) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["FIELD", "VALUE"]);

    // Use long_name with short_name as fallback (IDX stocks often only have shortName)
    let name = if !profile.long_name.is_empty() {
        &profile.long_name
    } else {
        &profile.short_name
    };

    let add_if_present = |t: &mut Table, label: &str, value: &str| {
        if !value.is_empty() {
            t.add_row(vec![cell(label), cell(value)]);
        }
    };

    add_if_present(&mut table, "Symbol", &profile.symbol);
    add_if_present(&mut table, "Name", name);
    add_if_present(&mut table, "Sector", &profile.sector);
    add_if_present(&mut table, "Industry", &profile.industry);
    add_if_present(&mut table, "Website", &profile.website);
    add_if_present(&mut table, "Country", &profile.country);
    add_if_present(&mut table, "City", &profile.city);
    add_if_present(&mut table, "Phone", &profile.phone);
    if profile.employees > 0 {
        table.add_row(vec![cell("Employees"), cell(profile.employees.to_string())]);
    }
    if !profile.description.is_empty() {
        // Truncate long descriptions for table display
        let desc = truncate_chars(&profile.description, 200);
        table.add_row(vec![cell("Description"), cell(desc)]);
    }
    if !profile.officers.is_empty() {
        table.add_row(vec![
            cell("Executives"),
            cell(
                profile
                    .officers
                    .iter()
                    .take(5)
                    .map(|o| format!("{} ({})", o.name, o.title))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn print_financials(fin: &FinancialStatements) -> Result<(), IdxError> {
    let print_section = |label: &str, section: &idx_core::api::types::StatementSection| {
        println!(
            "\n── {label} ({}) ──",
            terminal_safe(&format_table_date(&section.end_date))
        );
        let mut t = Table::new();
        let value_header = format!("VALUE ({})", terminal_safe(&section.currency));
        t.load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["LINE ITEM", value_header.as_str()]);

        let mut entries: Vec<(String, &f64)> = section
            .values
            .iter()
            .map(|(key, value)| (humanize_metric_key(key), value))
            .collect();
        entries.sort_by(|(left, _), (right, _)| left.cmp(right));

        for (label, value) in entries {
            t.add_row(vec![cell(label), cell(format_idr(*value as i64))]);
        }
        println!("{t}");
    };

    if let Some(income) = &fin.income_statement {
        print_section("Income Statement", income);
    }
    if let Some(balance) = &fin.balance_sheet {
        print_section("Balance Sheet", balance);
    }
    if let Some(cf) = &fin.cash_flow {
        print_section("Cash Flow", cf);
    }

    if fin.income_statement.is_none() && fin.balance_sheet.is_none() && fin.cash_flow.is_none() {
        println!("No financial statement data available for this stock.");
    }
    Ok(())
}

pub fn print_earnings(report: &EarningsReport) -> Result<(), IdxError> {
    if report.history.is_empty() && report.forecast.is_empty() {
        println!("No earnings data available for this stock.");
        return Ok(());
    }

    if !report.history.is_empty() {
        println!("── Earnings History ──");
        let mut history_table = Table::new();
        history_table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "PERIOD",
                "EPS ACT",
                "EPS FC",
                "SURPRISE",
                "SURPRISE%",
                "REV ACT",
                "DATE",
            ]);

        for row in &report.history {
            add_earnings_history_row(&mut history_table, row);
        }
        println!("{history_table}");
    }

    if !report.forecast.is_empty() {
        println!("\n── Earnings Forecast ──");
        let mut forecast_table = Table::new();
        forecast_table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["PERIOD", "EPS FC", "REV FC", "DATE"]);

        for row in &report.forecast {
            add_earnings_forecast_row(&mut forecast_table, row);
        }
        println!("{forecast_table}");
    }

    Ok(())
}

pub fn print_sentiment(data: &SentimentData) -> Result<(), IdxError> {
    if data.statistics.is_empty() {
        println!("No sentiment votes for {}.", terminal_safe(&data.symbol));
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["RANGE", "BULLISH", "BEARISH", "NEUTRAL"]);
    for row in &data.statistics {
        table.add_row(vec![
            cell(&row.time_range),
            cell(row.bullish),
            cell(row.bearish),
            cell(row.neutral),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn print_insights(data: &InsightData) -> Result<(), IdxError> {
    println!("{}", terminal_safe(&data.summary));
    if !data.last_updated.is_empty() {
        println!("Last updated: {}", terminal_safe(&data.last_updated));
    }
    if !data.highlights.is_empty() {
        println!("Highlights:");
        for h in &data.highlights {
            println!("- {}", terminal_safe(h));
        }
    }
    if !data.risks.is_empty() {
        println!("Risks:");
        for r in &data.risks {
            println!("- {}", terminal_safe(r));
        }
    }
    Ok(())
}

pub fn print_news(items: &[NewsItem]) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["TITLE", "ABOUT", "PROVIDER", "DATE", "URL"]);
    for item in items {
        table.add_row(vec![
            cell(&item.title),
            cell(item.relevance.as_str()),
            cell(&item.provider),
            cell(&item.published_at),
            cell(truncate_url(&item.url)),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn add_earnings_history_row(table: &mut Table, row: &EarningsData) {
    table.add_row(vec![
        cell(format_earnings_period(&row.period_type)),
        cell(format_float(row.eps_actual, 2)),
        cell(format_float(row.eps_forecast, 2)),
        cell(format_float(row.eps_surprise, 2)),
        cell(format_float(row.eps_surprise_pct, 2)),
        cell(format_idr_option_from_f64(row.revenue_actual)),
        cell(format_table_date_option(
            row.earning_release_date.as_deref(),
        )),
    ]);
}

fn add_earnings_forecast_row(table: &mut Table, row: &EarningsData) {
    table.add_row(vec![
        cell(format_earnings_period(&row.period_type)),
        cell(format_float(row.eps_forecast, 2)),
        cell(format_idr_option_from_f64(row.revenue_forecast)),
        cell(format_table_date_option(
            row.earning_release_date.as_deref(),
        )),
    ]);
}

fn truncate_url(url: &str) -> String {
    truncate_chars(url, 72)
}

#[cfg(test)]
mod tests {
    use super::{
        format_earnings_period, format_idr, format_signal, format_table_date, format_u64,
        humanize_metric_key, stale_quote_notes,
    };
    use idx_core::analysis::signals::Signal;
    use idx_core::api::types::Quote;

    fn quote_as_of(symbol: &str, as_of: &str) -> Quote {
        Quote {
            symbol: symbol.into(),
            price: 204,
            change: None,
            change_pct: None,
            volume: None,
            market_cap: None,
            week52_high: None,
            week52_low: None,
            week52_position: None,
            range_signal: None,
            prev_close: None,
            avg_volume: None,
            as_of: Some(chrono::DateTime::parse_from_rfc3339(as_of).unwrap()),
        }
    }

    #[test]
    fn notes_only_quotes_older_than_a_week() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-02T10:00:00Z")
            .unwrap()
            .to_utc();
        let quotes = [
            quote_as_of("WIKA.JK", "2025-02-17T16:14:58+07:00"),
            quote_as_of("BBCA.JK", "2026-10-02T16:15:00+07:00"),
            quote_as_of("TLKM.JK", "2026-09-26T16:15:00+07:00"),
        ];
        assert_eq!(
            stale_quote_notes(&quotes, now),
            vec!["Note: WIKA.JK last traded on 2025-02-17; this price is not current."]
        );
    }

    #[test]
    fn formats_idr_numbers() {
        assert_eq!(format_idr(9875), "9,875");
        assert_eq!(format_idr(-433_471_000_000), "-433,471,000,000");
        assert_eq!(format_u64(1_215_200_000_000_000), "1,215,200,000,000,000");
    }

    #[test]
    fn formats_plain_signal_labels() {
        assert_eq!(format_signal(Signal::Bullish, true, false), "Bullish");
        assert_eq!(format_signal(Signal::Bearish, true, true), "BEARISH");
    }

    #[test]
    fn humanizes_metric_keys_for_table_output() {
        assert_eq!(humanize_metric_key("netIncome"), "Net Income");
        assert_eq!(
            humanize_metric_key("basicEPSExcludingExtraordinaryItems"),
            "Basic EPS Excluding Extraordinary Items"
        );
        assert_eq!(
            humanize_metric_key("cashAndShortTermInvestments"),
            "Cash And Short Term Investments"
        );
    }

    #[test]
    fn formats_table_dates_without_timestamps() {
        assert_eq!(format_table_date("2025-03-31T00:00:00Z"), "2025-03-31");
        assert_eq!(format_table_date("2025-12-31"), "2025-12-31");
    }

    #[test]
    fn formats_earnings_periods_for_display() {
        assert_eq!(format_earnings_period("2025"), "FY2025");
        assert_eq!(format_earnings_period("Q12026"), "Q1 2026");
        assert_eq!(format_earnings_period(""), "-");
    }
}
