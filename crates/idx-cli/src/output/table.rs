use crate::output::style::Paint;
use crate::output::text::{cell, painted, terminal_safe, truncate_chars};
use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};

use idx_core::analysis::bandar::{
    ACCUMULATION_BPS, BIG_BPS, BandarReport, BandarSignal, METHOD as BANDAR_METHOD,
};
use idx_core::analysis::fundamental::{
    FundamentalReport, GrowthReport, RiskReport, ValuationReport,
};
use idx_core::analysis::report::TechnicalReport;
use idx_core::analysis::signals::Signal;
use idx_core::api::types::{
    BookLevel, CompanyProfile, EarningsData, EarningsReport, FinancialStatements, InsightData,
    Intraday, NewsItem, Ohlc, OrderBook, Quote, SentimentData,
};
use idx_core::broker::types::{BrokerBoard, BrokerFlow, BrokerInvestor, BrokerKind, BrokerSummary};
use idx_core::error::IdxError;
use idx_core::flow::types::{FlowSource, ForeignFlow};

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
        let pct_cell = pct_cell(q.change_pct, no_color);
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
/// stock; say so under the table instead of letting it pass as current. One
/// from before the latest session is usually the provider lagging the close.
fn stale_quote_notes(quotes: &[Quote], now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    quotes
        .iter()
        .filter_map(|q| {
            let as_of = q.as_of?;
            if now.signed_duration_since(as_of) > chrono::Duration::days(7) {
                Some(format!(
                    "Note: {} last traded on {}; this price is not current.",
                    q.symbol,
                    as_of.date_naive()
                ))
            } else if q.behind_session {
                Some(format!(
                    "Note: {} last traded on {}, before the latest IDX session; \
                     this price may be out of date.",
                    q.symbol,
                    as_of.date_naive()
                ))
            } else {
                None
            }
        })
        .collect()
}

fn pct_cell(change_pct: Option<f64>, no_color: bool) -> Cell {
    match change_pct {
        None => cell("-"),
        Some(pct) if no_color => cell(format!("{pct:+.2}%")),
        Some(pct) if pct >= 0.0 => cell(format!("{pct:+.2}%")).fg(Color::Green),
        Some(pct) => cell(format!("{pct:+.2}%")).fg(Color::Red),
    }
}

/// Printed under every Stockbit table: what the data is and is not.
const STOCKBIT_SOURCE_NOTE: &str = "Source: Stockbit's public page, delayed at least 10 \
minutes (Stockbit's terms); bid/offer only while the market is open.";
/// Under `--provider stockbit` quotes, which may come from either source.
const STOCKBIT_QUOTE_NOTE: &str = "Source: Stockbit, your session when logged in (`idx \
stockbit status`), else its public page, delayed at least 10 minutes (Stockbit's terms); \
bid/offer only while the market is open.";
/// Under the order book, which comes from the user's own session.
const STOCKBIT_SESSION_NOTE: &str =
    "Source: your Stockbit session; for personal, non-commercial use (Stockbit's terms).";

fn opt_idr(value: Option<i64>) -> String {
    value.map(format_idr).unwrap_or_else(|| "-".to_string())
}

fn wib_minute(time: chrono::DateTime<chrono::FixedOffset>) -> String {
    time.format("%Y-%m-%d %H:%M").to_string()
}

/// Quotes from `--provider stockbit`: Stockbit has no market cap or 52-week
/// range, but a best bid/offer and the market's state.
pub fn print_stockbit_quotes(quotes: &[Quote], no_color: bool) -> Result<(), IdxError> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "SYMBOL", "PRICE", "CHG", "CHG%", "VOLUME", "BID", "OFFER", "MARKET", "AS OF",
        ]);
    for q in quotes {
        table.add_row(vec![
            cell(&q.symbol),
            cell(format_idr(q.price)),
            cell(
                q.change
                    .map(|c| format!("{c:+}"))
                    .unwrap_or_else(|| "-".to_string()),
            ),
            pct_cell(q.change_pct, no_color),
            cell(q.volume.map(format_u64).unwrap_or_else(|| "-".to_string())),
            cell(opt_idr(q.bid)),
            cell(opt_idr(q.offer)),
            cell(
                q.market_status
                    .as_deref()
                    .map(market_words)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            cell(q.as_of.map(wib_minute).unwrap_or_else(|| "-".to_string())),
        ]);
    }
    println!("{table}");
    for q in quotes {
        if let Some(note) = &q.session_note {
            println!(
                "Note: {}: {}",
                terminal_safe(&q.symbol),
                terminal_safe(note)
            );
        }
    }
    for note in stale_quote_notes(quotes, chrono::Utc::now()) {
        println!("{}", terminal_safe(&note));
    }
    println!("{STOCKBIT_QUOTE_NOTE}");
    Ok(())
}

/// Minutes listed under the intraday chart.
const INTRADAY_TAIL: usize = 5;
/// Widest the intraday sparkline gets, in characters.
const SPARKLINE_WIDTH: usize = 60;

pub fn print_intraday(day: &Intraday, no_color: bool) -> Result<(), IdxError> {
    let q = &day.quote;
    let change = match (q.change, q.change_pct) {
        (Some(c), Some(p)) => format!(" {c:+} ({p:+.2}%)"),
        (Some(c), None) => format!(" {c:+}"),
        _ => String::new(),
    };
    let change = match q.change {
        _ if no_color || change.is_empty() => change,
        Some(c) if c >= 0 => change.green(),
        _ => change.red(),
    };
    println!(
        "{}{change}",
        format!("{} {}", terminal_safe(&q.symbol), format_idr(q.price)).bold()
    );
    let mut facts = vec![
        format!("prev close {}", opt_idr(q.prev_close)),
        format!(
            "volume {}",
            q.volume.map(format_u64).unwrap_or_else(|| "-".to_string())
        ),
        format!(
            "market {}",
            terminal_safe(q.market_status.as_deref().unwrap_or("unknown"))
        ),
    ];
    if let Some(as_of) = q.as_of {
        facts.push(format!("last trade {} WIB", wib_minute(as_of)));
    }
    println!("{}", facts.join(" · "));
    if q.bid.is_some() || q.offer.is_some() {
        println!("bid {} · offer {}", opt_idr(q.bid), opt_idr(q.offer));
    }
    if let Some(note) = &q.session_note {
        println!("Note: {}", terminal_safe(note));
    }

    let prices: Vec<f64> = day.points.iter().map(|p| p.price).collect();
    if let (Some(first), Some(last)) = (day.points.first(), day.points.last()) {
        let (low, high) = prices
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        println!();
        println!("{}", sparkline(&prices, SPARKLINE_WIDTH));
        println!(
            "{} to {} WIB · low {} · high {} · {} minutes",
            first.time.format("%H:%M"),
            last.time.format("%H:%M"),
            format_price(low),
            format_price(high),
            day.points.len()
        );
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["TIME", "PRICE"]);
        let start = day.points.len().saturating_sub(INTRADAY_TAIL);
        for point in &day.points[start..] {
            table.add_row(vec![
                cell(point.time.format("%H:%M")),
                cell(format_price(point.price)),
            ]);
        }
        println!("{table}");
    } else if q.session_note.is_none() {
        println!("No trades this session.");
    }
    println!("{STOCKBIT_SOURCE_NOTE}");
    Ok(())
}

/// Stockbit's market state for people: `STATE_NAME_MARKET_CLOSED` reads
/// `closed`.
fn market_words(state: &str) -> String {
    state
        .strip_prefix("STATE_NAME_MARKET_")
        .unwrap_or(state)
        .replace('_', " ")
        .to_lowercase()
}

pub fn print_orderbook(book: &OrderBook, no_color: bool) -> Result<(), IdxError> {
    let change = match (book.change, book.change_pct) {
        (Some(c), Some(p)) => format!(" {c:+} ({p:+.2}%)"),
        (Some(c), None) => format!(" {c:+}"),
        _ => String::new(),
    };
    let change = match book.change {
        _ if no_color || change.is_empty() => change,
        Some(c) if c >= 0 => change.green(),
        _ => change.red(),
    };
    let name = book
        .name
        .as_deref()
        .map(|n| format!(" {}", terminal_safe(n)))
        .unwrap_or_default();
    println!(
        "{}{change}",
        format!(
            "{}{name} {}",
            terminal_safe(&book.symbol),
            format_idr(book.price)
        )
        .bold()
    );
    let count = |v: Option<u64>| v.map(format_u64).unwrap_or_else(|| "-".to_string());
    println!(
        "prev close {} · open {} · high {} · low {} · volume {} · value {} · {} trades",
        opt_idr(book.prev_close),
        opt_idr(book.open),
        opt_idr(book.high),
        opt_idr(book.low),
        count(book.volume),
        count(book.value),
        count(book.frequency)
    );
    if book.foreign_buy.is_some() || book.foreign_sell.is_some() {
        println!(
            "foreign buy {} · sell {} · net {}",
            opt_idr(book.foreign_buy),
            opt_idr(book.foreign_sell),
            opt_idr(book.foreign_net)
        );
    }
    match book.market_status.as_deref() {
        Some(idx_core::api::stockbit::exodus::MARKET_OPEN) => println!("market open"),
        Some(state) => println!(
            "market {}: the book as the market left it",
            terminal_safe(&market_words(state))
        ),
        None => println!("market state unknown: the book may not be live"),
    }
    if let Some(note) = &book.session_note {
        println!("Note: {}", terminal_safe(note));
    }
    if book.bids.is_empty() && book.offers.is_empty() {
        println!("No bids or offers.");
    } else {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["ORDERS", "LOTS", "BID", "OFFER", "LOTS", "ORDERS"]);
        let side = |level: Option<&BookLevel>| match level {
            Some(l) => [
                format_u64(l.orders),
                format_u64(l.shares / 100),
                format_idr(l.price),
            ],
            None => ["".to_string(), "".to_string(), "".to_string()],
        };
        for i in 0..book.bids.len().max(book.offers.len()) {
            let [bid_orders, bid_lots, bid] = side(book.bids.get(i));
            let [offer_orders, offer_lots, offer] = side(book.offers.get(i));
            table.add_row(vec![
                cell(bid_orders),
                cell(bid_lots),
                cell(bid),
                cell(offer),
                cell(offer_lots),
                cell(offer_orders),
            ]);
        }
        println!("{table}");
    }
    println!("{STOCKBIT_SESSION_NOTE}");
    Ok(())
}

/// Whole Rupiah for stocks, two decimals for an index level.
fn format_price(price: f64) -> String {
    let cents = (price * 100.0).round() as i64;
    if cents % 100 == 0 {
        format_idr(cents / 100)
    } else {
        format!("{}.{:02}", format_idr(cents / 100), (cents % 100).abs())
    }
}

/// One block per bucket of points (the bucket's last price), scaled from the
/// lowest to the highest price.
fn sparkline(prices: &[f64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let width = width.min(prices.len());
    if width == 0 {
        return String::new();
    }
    let (low, high) = prices
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    (1..=width)
        .map(|i| {
            let price = prices[i * prices.len() / width - 1];
            if high > low {
                BARS[((price - low) / (high - low) * 7.0).round() as usize]
            } else {
                BARS[3]
            }
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
            terminal_safe(&report.symbol),
            report.as_of
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
            .map(|report| terminal_safe(&report.symbol))
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

/// A figure at the precision the broker summary source publishes:
/// `841.2B`, `1.6M`, exact below a million.
fn format_compact(value: i64) -> String {
    let abs = value.unsigned_abs();
    let (unit, suffix) = match abs {
        1_000_000_000_000.. => (1e12, "T"),
        1_000_000_000.. => (1e9, "B"),
        1_000_000.. => (1e6, "M"),
        _ => return format_idr(value),
    };
    format!("{:.1}{suffix}", value as f64 / unit)
}

fn format_compact_option(value: Option<i64>) -> String {
    value.map(format_compact).unwrap_or_else(|| "-".to_string())
}

fn broker_cells(flow: Option<&BrokerFlow>) -> [Cell; 4] {
    match flow {
        Some(flow) => [
            cell(&flow.code),
            cell(format_compact_option(flow.lots)),
            cell(format_compact_option(flow.value)),
            cell(
                flow.average_price
                    .map(format_idr)
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ],
        None => [cell(""), cell(""), cell(""), cell("")],
    }
}

/// `2026-10-16`, or `2026-10-12 to 2026-10-16`, plus any filters.
pub(crate) fn broker_scope(
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    investor: BrokerInvestor,
    board: BrokerBoard,
) -> String {
    let mut scope = if from == to {
        from.to_string()
    } else {
        format!("{from} to {to}")
    };
    if investor != BrokerInvestor::All {
        scope.push_str(&format!(", {} investors", investor.as_str()));
    }
    if board != BrokerBoard::All {
        scope.push_str(&format!(", {} board", board.as_str()));
    }
    scope
}

/// What one investor side bought and sold, when a summary covers one side.
fn side_totals_line(
    investor: BrokerInvestor,
    board: BrokerBoard,
    buy_value: Option<i64>,
    sell_value: Option<i64>,
    foreign_net: Option<i64>,
) -> Option<String> {
    let side = match investor {
        BrokerInvestor::All => return None,
        BrokerInvestor::Foreign => "Foreign",
        BrokerInvestor::Domestic => "Domestic",
    };
    let buy = format_compact_option(buy_value);
    Some(match sell_value {
        Some(sell) => {
            let net = foreign_net
                .map(|net| match investor {
                    BrokerInvestor::Domestic => -net,
                    _ => net,
                })
                .map(|net| format!(" (net {:+})", format_compact(net)))
                .unwrap_or_default();
            format!(
                "{side} investors bought {buy} IDR and sold {} IDR{net}.",
                format_compact(sell)
            )
        }
        None => format!(
            "{side} investors bought {buy} IDR on the {} board; the source gives one side's \
selling only across all boards.",
            board.as_str()
        ),
    })
}

pub fn print_broker_summary(summary: &BrokerSummary) -> Result<(), IdxError> {
    println!(
        "{}",
        format!(
            "Broker summary for {}, {}",
            terminal_safe(&summary.symbol),
            broker_scope(summary.from, summary.to, summary.investor, summary.board)
        )
        .bold()
    );
    if summary.is_empty() {
        println!("No trades.");
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "#", "BUYER", "LOTS", "VALUE", "AVG", "SELLER", "LOTS", "VALUE", "AVG",
        ]);
    let rows = summary.buyers.len().max(summary.sellers.len());
    for index in 0..rows {
        let mut row = vec![cell(index + 1)];
        row.extend(broker_cells(summary.buyers.get(index)));
        row.extend(broker_cells(summary.sellers.get(index)));
        table.add_row(row);
    }
    println!("{table}");

    if summary.total_value.is_some() {
        let mut totals = format!(
            "Total {} IDR, {} lots",
            format_compact_option(summary.total_value),
            format_compact_option(summary.total_lots)
        );
        if let Some(average) = summary.average_price {
            totals.push_str(&format!(", average {}", format_idr(average)));
        }
        if let Some(net) = summary.foreign_net_value {
            totals.push_str(&format!("; foreign net {:+}", format_compact(net)));
        }
        println!("{totals}.");
    } else if let Some(line) = side_totals_line(
        summary.investor,
        summary.board,
        summary.buy_value,
        summary.sell_value,
        summary.foreign_net_value,
    ) {
        println!("{line}");
    }
    for (kind, label) in [
        (BrokerKind::Foreign, "Foreign brokers"),
        (BrokerKind::StateOwned, "State-owned brokers"),
    ] {
        let mut codes: Vec<&str> = summary
            .buyers
            .iter()
            .chain(&summary.sellers)
            .filter(|flow| flow.kind == Some(kind))
            .map(|flow| flow.code.as_str())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        if !codes.is_empty() {
            println!("{label}: {}.", terminal_safe(&codes.join(", ")));
        }
    }
    println!("Buyers and sellers are each ranked by lots; a broker can be on both sides.");
    if summary.rounded {
        println!("Note: the source rounds figures of a million and more to one decimal.");
    }
    Ok(())
}

fn format_bps_pct(bps: Option<i64>) -> String {
    bps.map(|bps| format!("{:+.1}%", bps as f64 / 100.0))
        .unwrap_or_else(|| "-".to_string())
}

fn format_bps_share(bps: Option<i64>) -> String {
    bps.map(|bps| format!("{:.1}%", bps as f64 / 100.0))
        .unwrap_or_else(|| "-".to_string())
}

fn format_bandar_signal(signal: Option<BandarSignal>, no_color: bool) -> String {
    let Some(signal) = signal else {
        return "-".to_string();
    };
    let label = signal.as_str().replace('_', " ");
    if no_color {
        return label;
    }
    match signal {
        BandarSignal::BigAccumulation | BandarSignal::Accumulation => label.green(),
        BandarSignal::BigDistribution | BandarSignal::Distribution => label.red(),
        BandarSignal::Neutral => label,
    }
}

fn bandar_method_note(top: u8) -> String {
    format!(
        "Method {BANDAR_METHOD}: the top {top} buyers' share of the value bought minus the top \
{top} sellers' share of the value sold; {}% or more reads accumulation, {}% or more big \
(negative: distribution). Brokers trade for many clients, so this is a heuristic, not advice.",
        ACCUMULATION_BPS / 100,
        BIG_BPS / 100
    )
}

pub fn print_bandar_report(report: &BandarReport, no_color: bool) -> Result<(), IdxError> {
    println!(
        "{}",
        format!(
            "Bandar reading for {}, {}",
            terminal_safe(&report.symbol),
            broker_scope(report.from, report.to, report.investor, report.board)
        )
        .bold()
    );
    // Brokers on either side mean trades, even when a total is unavailable.
    if report
        .tiers
        .iter()
        .all(|tier| tier.buy_value == Some(0) && tier.sell_value == Some(0))
    {
        println!("No trades.");
        return Ok(());
    }
    println!(
        "Signal (top {}): {}, {} of value",
        report.top,
        format_bandar_signal(report.signal, no_color),
        format_bps_pct(report.imbalance_bps)
    );
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "TOP", "BUY", "SELL", "NET", "NET %", "BUY AVG", "SELL AVG", "SIGNAL",
        ]);
    for tier in &report.tiers {
        table.add_row(vec![
            cell(tier.top),
            cell(format_compact_option(tier.buy_value)),
            cell(format_compact_option(tier.sell_value)),
            cell(format_compact_option(tier.imbalance_value)),
            cell(format_bps_pct(tier.imbalance_bps)),
            cell(
                tier.buy_average
                    .map(format_idr)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            cell(
                tier.sell_average
                    .map(format_idr)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            painted(format_bandar_signal(tier.signal, no_color)),
        ]);
    }
    println!("{table}");
    if let Some(line) = side_totals_line(
        report.investor,
        report.board,
        report.buy_value,
        report.sell_value,
        report.foreign_net_value,
    ) {
        println!("{line}");
    } else {
        let mut totals = format!("Total {} IDR", format_compact_option(report.total_value));
        if let Some(average) = report.average_price {
            totals.push_str(&format!(", average {}", format_idr(average)));
        }
        if let Some(net) = report.foreign_net_value {
            totals.push_str(&format!(
                "; foreign net {:+} ({} of value)",
                format_compact(net),
                format_bps_pct(report.foreign_net_bps)
            ));
        }
        println!("{totals}.");
    }
    if !report.both_sides.is_empty() {
        println!(
            "On both sides: {}; their gross figures overstate which way they traded.",
            terminal_safe(&report.both_sides.join(", "))
        );
    }
    println!("{}", bandar_method_note(report.top));
    if report.rounded {
        println!("Note: the source rounds figures of a million and more to one decimal.");
    }
    Ok(())
}

pub fn print_bandar_screen(reports: &[BandarReport], no_color: bool) -> Result<(), IdxError> {
    let top = reports.first().map(|report| report.top).unwrap_or_default();
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "SYMBOL".to_string(),
            "DATES".to_string(),
            "SIGNAL".to_string(),
            format!("TOP {top} NET %"),
            "BUY SHARE".to_string(),
            "SELL SHARE".to_string(),
            "VALUE".to_string(),
            "FOREIGN NET".to_string(),
        ]);
    for report in reports {
        let headline = report.tiers.iter().find(|tier| tier.top == report.top);
        table.add_row(vec![
            cell(&report.symbol),
            cell(broker_scope(
                report.from,
                report.to,
                BrokerInvestor::All,
                BrokerBoard::All,
            )),
            painted(format_bandar_signal(report.signal, no_color)),
            cell(format_bps_pct(report.imbalance_bps)),
            cell(format_bps_share(
                headline.and_then(|tier| tier.buy_share_bps),
            )),
            cell(format_bps_share(
                headline.and_then(|tier| tier.sell_share_bps),
            )),
            cell(format_compact_option(report.total_value)),
            cell(
                report
                    .foreign_net_value
                    .map(|net| format!("{:+}", format_compact(net)))
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ]);
    }
    println!("{table}");
    if let Some(first) = reports.first()
        && (first.investor, first.board) != (BrokerInvestor::All, BrokerBoard::All)
    {
        println!(
            "Counting {} investors on the {} board.",
            first.investor.as_str(),
            first.board.as_str()
        );
    }
    println!("{}", bandar_method_note(top));
    Ok(())
}

fn format_signed_compact(value: Option<i64>) -> String {
    value
        .map(|value| {
            let text = format_compact(value);
            if value > 0 { format!("+{text}") } else { text }
        })
        .unwrap_or_else(|| "-".to_string())
}

pub fn print_foreign_flow(flow: &ForeignFlow) -> Result<(), IdxError> {
    let span = if flow.from == flow.to {
        flow.from.to_string()
    } else {
        format!("{} to {}", flow.from, flow.to)
    };
    let sessions = flow.days.len();
    println!(
        "{}",
        format!(
            "Foreign flow for {}, {span} ({sessions} session{}, {})",
            terminal_safe(&flow.symbol),
            if sessions == 1 { "" } else { "s" },
            match flow.source {
                FlowSource::Ipot => "IPOT",
                FlowSource::Idx => "IDX",
            }
        )
        .bold()
    );
    match flow.source {
        FlowSource::Ipot => print_ipot_flow(flow),
        FlowSource::Idx => print_idx_flow(flow),
    }
    if flow.rounded {
        println!("Note: the source rounds figures of a million and more to one decimal.");
    }
    Ok(())
}

fn print_ipot_flow(flow: &ForeignFlow) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "DATE",
            "VALUE",
            "FOREIGN BUY",
            "FOREIGN SELL",
            "FOREIGN NET",
            "FOREIGN %",
        ]);
    for day in &flow.days {
        table.add_row(vec![
            cell(day.date),
            cell(format_compact_option(day.value)),
            cell(format_compact_option(day.foreign_buy_value)),
            cell(format_compact_option(day.foreign_sell_value)),
            cell(format_signed_compact(day.foreign_net_value)),
            cell(format_bps_share(day.foreign_share_bps)),
        ]);
    }
    println!("{table}");
    let mut summary = format!(
        "Foreign investors bought {} IDR and sold {} IDR: net {}",
        format_compact_option(flow.foreign_buy_value),
        format_compact_option(flow.foreign_sell_value),
        format_signed_compact(flow.foreign_net_value)
    );
    if let Some(share) = flow.foreign_share_bps {
        summary.push_str(&format!(
            "; they made up {} of trading",
            format_bps_share(Some(share))
        ));
    }
    println!("{summary}.");
    println!(
        "Values in IDR across all boards. For exact share counts, the close and the non-regular \
share, use --source idx (needs curl-impersonate)."
    );
}

fn print_idx_flow(flow: &ForeignFlow) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "DATE",
            "CLOSE",
            "CHG%",
            "VALUE",
            "FOREIGN BUY",
            "FOREIGN SELL",
            "FOREIGN NET",
            "NET VALUE",
            "NON-REG",
        ]);
    for day in &flow.days {
        table.add_row(vec![
            cell(day.date),
            cell(day.close.map(format_idr).unwrap_or_else(|| "-".to_string())),
            cell(format_bps_pct(day.change_bps)),
            cell(format_compact_option(day.value)),
            cell(format_compact_option(day.foreign_buy)),
            cell(format_compact_option(day.foreign_sell)),
            cell(format_signed_compact(day.foreign_net)),
            cell(format_signed_compact(day.foreign_net_value)),
            cell(format_bps_share(day.non_regular_bps)),
        ]);
    }
    println!("{table}");
    let mut summary = format!(
        "Net foreign: {} shares, about {} IDR",
        format_signed_compact(flow.foreign_net),
        format_signed_compact(flow.foreign_net_value)
    );
    if let Some(share) = flow.foreign_share_bps {
        summary.push_str(&format!(
            "; foreign investors made up {} of trading",
            format_bps_share(Some(share))
        ));
    }
    println!("{summary}.");
    println!(
        "Volume, value and foreign figures are the regular board, volumes in shares. Net value \
is estimated at each session's average price; NON-REG is the share of all value traded \
outside the regular board (negotiated and cash)."
    );
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
            behind_session: false,
            bid: None,
            offer: None,
            market_status: None,
            session_note: None,
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
    fn notes_quotes_behind_the_latest_session() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-16T16:53:00+07:00")
            .unwrap()
            .to_utc();
        let mut behind = quote_as_of("BBCA.JK", "2026-10-15T16:15:00+07:00");
        behind.behind_session = true;
        let mut suspended = quote_as_of("WIKA.JK", "2025-02-17T16:14:58+07:00");
        suspended.behind_session = true;
        let quotes = [
            behind,
            suspended,
            quote_as_of("BBRI.JK", "2026-10-16T16:15:00+07:00"),
        ];
        assert_eq!(
            stale_quote_notes(&quotes, now),
            vec![
                "Note: BBCA.JK last traded on 2026-10-15, before the latest IDX session; \
                 this price may be out of date.",
                "Note: WIKA.JK last traded on 2025-02-17; this price is not current.",
            ]
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
