use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use owo_colors::OwoColorize;

use crate::analysis::fundamental::{FundamentalReport, GrowthReport, RiskReport, ValuationReport};
use crate::analysis::signals::Signal;
use crate::api::types::{
    CompanyProfile, EarningsData, EarningsReport, FinancialStatements, InsightData, NewsItem, Ohlc,
    Quote, SentimentData,
};
use crate::error::IdxError;
use crate::output::TechnicalReport;

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

pub fn print_growth(symbol: &str, report: &GrowthReport, no_color: bool) -> Result<(), IdxError> {
    println!("{}", format!("Growth Analysis for {symbol}").bold());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        Cell::new("Revenue Growth"),
        Cell::new(format_pct(report.revenue_growth_pct)),
        Cell::new(format_growth_signal(&report.revenue_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("Earnings Growth"),
        Cell::new(format_pct(report.earnings_growth_pct)),
        Cell::new(format_growth_signal(&report.earnings_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("Overall"),
        Cell::new("-"),
        Cell::new(format_growth_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_valuation(
    symbol: &str,
    report: &ValuationReport,
    no_color: bool,
) -> Result<(), IdxError> {
    println!("{}", format!("Valuation Analysis for {symbol}").bold());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        Cell::new("P/E (Trailing)"),
        Cell::new(format_opt_f64(report.pe_trailing, 2)),
        Cell::new(format_valuation_signal(&report.pe_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("P/E (Forward)"),
        Cell::new(format_opt_f64(report.pe_forward, 2)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("Price/Book"),
        Cell::new(format_opt_f64(report.pb, 2)),
        Cell::new(format_valuation_signal(&report.pb_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("ROE"),
        Cell::new(format_pct(report.roe_pct)),
        Cell::new(format_valuation_signal(&report.roe_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("Net Margin"),
        Cell::new(format_pct(report.net_margin_pct)),
        Cell::new(format_valuation_signal(&report.margin_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("EV/EBITDA"),
        Cell::new(format_opt_f64(report.ev_ebitda, 2)),
        Cell::new(format_valuation_signal(&report.ev_ebitda_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("Overall"),
        Cell::new("-"),
        Cell::new(format_valuation_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_risk(symbol: &str, report: &RiskReport, no_color: bool) -> Result<(), IdxError> {
    println!("{}", format!("Risk Analysis for {symbol}").bold());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["METRIC", "VALUE", "SIGNAL"]);

    table.add_row(vec![
        Cell::new("Debt/Equity"),
        Cell::new(format_opt_f64(report.debt_to_equity, 2)),
        Cell::new(format_risk_signal(&report.de_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("Current Ratio"),
        Cell::new(format_opt_f64(report.current_ratio, 2)),
        Cell::new(format_risk_signal(&report.current_ratio_signal, no_color)),
    ]);
    table.add_row(vec![
        Cell::new("ROA"),
        Cell::new(format_pct(report.roa_pct)),
        Cell::new("-"),
    ]);
    table.add_row(vec![
        Cell::new("Overall"),
        Cell::new("-"),
        Cell::new(format_risk_signal(&report.overall_signal, no_color)),
    ]);

    println!("{table}");
    Ok(())
}

pub fn print_fundamental(report: &FundamentalReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!("Fundamental Analysis for {}", report.symbol).bold()
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

    let mut header = vec![Cell::new("METRIC")];
    header.extend(reports.iter().map(|report| Cell::new(&report.symbol)));
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

    if trimmed.len() == 6
        && trimmed.starts_with('Q')
        && trimmed[1..2].chars().all(|ch| ch.is_ascii_digit())
        && trimmed[2..].chars().all(|ch| ch.is_ascii_digit())
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
    if no_color {
        return signal.to_string();
    }

    if positive.contains(&signal) {
        signal.green().to_string()
    } else if negative.contains(&signal) {
        signal.red().to_string()
    } else {
        signal.yellow().to_string()
    }
}

fn add_compare_row(table: &mut Table, label: &str, values: Vec<String>) {
    let mut row = vec![Cell::new(label)];
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
            t.add_row(vec![Cell::new(label), Cell::new(value)]);
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
        table.add_row(vec![
            Cell::new("Employees"),
            Cell::new(profile.employees.to_string()),
        ]);
    }
    if !profile.description.is_empty() {
        // Truncate long descriptions for table display
        let desc = if profile.description.len() > 200 {
            format!("{}...", &profile.description[..200])
        } else {
            profile.description.clone()
        };
        table.add_row(vec![Cell::new("Description"), Cell::new(desc)]);
    }
    if !profile.officers.is_empty() {
        table.add_row(vec![
            Cell::new("Executives"),
            Cell::new(
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
    let print_section = |label: &str, section: &crate::api::types::StatementSection| {
        println!("\n── {label} ({}) ──", format_table_date(&section.end_date));
        let mut t = Table::new();
        let value_header = format!("VALUE ({})", section.currency);
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
            t.add_row(vec![Cell::new(label), Cell::new(format_idr(*value as i64))]);
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
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["RANGE", "BULLISH", "BEARISH", "NEUTRAL"]);
    for row in &data.statistics {
        table.add_row(vec![
            Cell::new(&row.time_range),
            Cell::new(row.bullish),
            Cell::new(row.bearish),
            Cell::new(row.neutral),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn print_insights(data: &InsightData) -> Result<(), IdxError> {
    println!("{}", data.summary);
    if !data.last_updated.is_empty() {
        println!("Last updated: {}", data.last_updated);
    }
    if !data.highlights.is_empty() {
        println!("Highlights:");
        for h in &data.highlights {
            println!("- {h}");
        }
    }
    if !data.risks.is_empty() {
        println!("Risks:");
        for r in &data.risks {
            println!("- {r}");
        }
    }
    Ok(())
}

pub fn print_news(items: &[NewsItem]) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_header(vec!["TITLE", "PROVIDER", "DATE", "URL"]);
    for item in items {
        table.add_row(vec![
            Cell::new(&item.title),
            Cell::new(&item.provider),
            Cell::new(&item.published_at),
            Cell::new(truncate_url(&item.url)),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn add_earnings_history_row(table: &mut Table, row: &EarningsData) {
    table.add_row(vec![
        Cell::new(format_earnings_period(&row.period_type)),
        Cell::new(format_float(row.eps_actual, 2)),
        Cell::new(format_float(row.eps_forecast, 2)),
        Cell::new(format_float(row.eps_surprise, 2)),
        Cell::new(format_float(row.eps_surprise_pct, 2)),
        Cell::new(format_idr_option_from_f64(row.revenue_actual)),
        Cell::new(format_table_date_option(
            row.earning_release_date.as_deref(),
        )),
    ]);
}

fn add_earnings_forecast_row(table: &mut Table, row: &EarningsData) {
    table.add_row(vec![
        Cell::new(format_earnings_period(&row.period_type)),
        Cell::new(format_float(row.eps_forecast, 2)),
        Cell::new(format_idr_option_from_f64(row.revenue_forecast)),
        Cell::new(format_table_date_option(
            row.earning_release_date.as_deref(),
        )),
    ]);
}

fn truncate_url(url: &str) -> String {
    if url.len() > 72 {
        format!("{}...", &url[..72])
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_earnings_period, format_idr, format_signal, format_table_date, format_u64,
        humanize_metric_key,
    };
    use crate::analysis::signals::Signal;

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
