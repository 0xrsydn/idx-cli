use super::raw_types::{
    IndustryMetric, KeyRatios, MsnQuote, RawChartResponse, RawChartSeries, RawEarningsData,
    RawEarningsResponse, RawEquity, RawFinancialStatement, RawInsight, RawInsightItem,
    RawLocalizedAttribute, RawNewsFeed, RawScreenerResponse, RawSentiment, RawStatementSection,
};
use super::symbols::{normalized_symbol, ticker_from_symbol};
use crate::api::types::{
    CompanyProfile, EarningsData, EarningsReport, FinancialStatements, Fundamentals, InsightData,
    InstrumentInfo, NewsItem, NewsRelevance, Officer, Ohlc, Quote, SentimentData, SentimentPeriod,
    StatementSection,
};
use crate::error::IdxError;
use chrono::DateTime;
use std::collections::HashMap;

pub(super) fn parse_quote(symbol: &str, quotes: &[MsnQuote]) -> Result<Quote, IdxError> {
    let quote = quotes.first().ok_or(IdxError::ProviderUnavailable)?;
    let Some(raw_price) = quote.price else {
        // MSN still lists suspended/untraded names (symbol, name, market cap)
        // but sends no price. That is a real instrument with no market data.
        if quote.symbol.is_some() {
            return Err(IdxError::NoMarketData(format!(
                "{symbol} is listed but has no price from MSN (it may be suspended)"
            )));
        }
        return Err(IdxError::SymbolNotFound(symbol.to_string()));
    };
    let prev_close = quote.price_previous_close.and_then(round_price);
    let price = round_price(raw_price).ok_or_else(|| {
        IdxError::ParseError(format!(
            "MSN sent an implausible price for {symbol}: {raw_price}"
        ))
    })?;
    let change = prev_close
        .map(|previous| price - previous)
        .or_else(|| quote.price_change.and_then(round_price));

    let ticker = quote
        .symbol
        .as_deref()
        .and_then(ticker_from_symbol)
        .unwrap_or_else(|| ticker_from_symbol(symbol).unwrap_or_default());

    let (week52_position, range_signal) = match (quote.price_52w_low, quote.price_52w_high) {
        (Some(low), Some(high)) if high > low => {
            let position = (raw_price - low) / (high - low);
            let signal = if position > 0.66 {
                Some("upper".to_string())
            } else if position < 0.33 {
                Some("lower".to_string())
            } else {
                Some("middle".to_string())
            };
            (Some(position), signal)
        }
        _ => (None, None),
    };

    Ok(Quote {
        symbol: normalized_symbol(symbol, &ticker),
        price,
        change,
        change_pct: quote.price_change_percent,
        volume: round_u64(quote.accumulated_volume),
        market_cap: round_u64(quote.market_cap),
        week52_high: quote.price_52w_high.and_then(round_price),
        week52_low: quote.price_52w_low.and_then(round_price),
        week52_position,
        range_signal,
        prev_close,
        avg_volume: round_u64(quote.average_volume),
        as_of: crate::api::parse_as_of(quote.time_last_traded.as_deref()),
    })
}

pub(super) fn parse_fundamentals(
    ratios: &[KeyRatios],
    quote: Option<&MsnQuote>,
) -> Result<Fundamentals, IdxError> {
    let ratios = ratios.first().ok_or(IdxError::ProviderUnavailable)?;
    let metrics = &ratios.company_metrics;
    if metrics.is_empty() || !metrics.iter().any(metric_has_supported_values) {
        return Err(IdxError::Unsupported(
            "company fundamentals unavailable from MSN; industry fallback is disabled".into(),
        ));
    }

    Ok(Fundamentals {
        trailing_pe: best_metric_value(metrics, |metric| metric.price_to_earnings_ratio),
        forward_pe: best_metric_value(metrics, |metric| metric.forward_price_to_eps),
        price_to_book: best_metric_value(metrics, |metric| metric.price_to_book_ratio),
        return_on_equity: best_metric_value(metrics, |metric| normalize_percentish(metric.roe)),
        profit_margins: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.profit_margin.or(metric.net_margin))
        }),
        return_on_assets: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.roa_ttm.or(metric.return_on_asset_current))
        }),
        revenue_growth: best_metric_value(metrics, |metric| {
            normalize_percentish(metric.revenue_ytd_ytd.or(metric.revenue_growth_rate))
        }),
        earnings_growth: best_metric_value(metrics, |metric| {
            normalize_percentish(
                metric
                    .net_income_ytd_ytd_growth_rate
                    .or(metric.earnings_growth_rate),
            )
        }),
        debt_to_equity: best_metric_value(metrics, |metric| metric.debt_to_equity_ratio),
        current_ratio: best_metric_value(metrics, |metric| {
            sanitize_current_ratio(metric.current_ratio)
        }),
        enterprise_value: None,
        ebitda: None,
        market_cap: quote.and_then(|item| round_u64(item.market_cap)),
    })
}

fn metric_has_supported_values(metric: &IndustryMetric) -> bool {
    [
        metric
            .price_to_earnings_ratio
            .filter(|value| value.is_finite()),
        metric
            .forward_price_to_eps
            .filter(|value| value.is_finite()),
        metric.price_to_book_ratio.filter(|value| value.is_finite()),
        normalize_percentish(metric.roe),
        normalize_percentish(metric.profit_margin.or(metric.net_margin)),
        normalize_percentish(metric.roa_ttm.or(metric.return_on_asset_current)),
        normalize_percentish(metric.revenue_ytd_ytd.or(metric.revenue_growth_rate)),
        normalize_percentish(
            metric
                .net_income_ytd_ytd_growth_rate
                .or(metric.earnings_growth_rate),
        ),
        metric
            .debt_to_equity_ratio
            .filter(|value| value.is_finite()),
        sanitize_current_ratio(metric.current_ratio),
    ]
    .into_iter()
    .any(|value| value.is_some())
}

fn best_metric_value<T: Copy>(
    metrics: &[IndustryMetric],
    extractor: impl Fn(&IndustryMetric) -> Option<T>,
) -> Option<T> {
    metrics
        .iter()
        .filter_map(|metric| extractor(metric).map(|value| (metric_rank(metric), value)))
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, value)| value)
}

fn metric_rank(metric: &IndustryMetric) -> (i32, i32) {
    (
        metric
            .year
            .as_deref()
            .and_then(|year| year.parse::<i32>().ok())
            .unwrap_or(i32::MIN),
        metric_period_priority(metric.fiscal_period_type.as_deref()),
    )
}

fn metric_period_priority(period: Option<&str>) -> i32 {
    match period.map(|value| value.trim()) {
        Some(value) if value.eq_ignore_ascii_case("TTM") => 7,
        Some(value)
            if value.eq_ignore_ascii_case("ANNUAL")
                || value.eq_ignore_ascii_case("FY")
                || value.eq_ignore_ascii_case("YEAR") =>
        {
            6
        }
        Some(value) if value.eq_ignore_ascii_case("Q4") => 5,
        Some(value) if value.eq_ignore_ascii_case("Q3") => 4,
        Some(value) if value.eq_ignore_ascii_case("Q2") => 3,
        Some(value) if value.eq_ignore_ascii_case("Q1") => 2,
        Some(value) if value.eq_ignore_ascii_case("NTM") => 1,
        _ => 0,
    }
}

fn normalize_percentish(value: Option<f64>) -> Option<f64> {
    value.and_then(|number| {
        if !number.is_finite() {
            None
        } else if number.abs() > 1.0 {
            Some(number / 100.0)
        } else {
            Some(number)
        }
    })
}

fn sanitize_current_ratio(value: Option<f64>) -> Option<f64> {
    value.filter(|number| number.is_finite() && *number >= 0.01)
}

/// Prices are IDR; anything non-finite, negative, or above this is not a
/// real quote, and treating it as missing keeps arithmetic from overflowing.
const MAX_PRICE: f64 = 1e12;

fn round_price(value: f64) -> Option<i64> {
    (value.is_finite() && (0.0..=MAX_PRICE).contains(&value)).then(|| value.round() as i64)
}

fn round_u64(value: Option<f64>) -> Option<u64> {
    value.and_then(|number| {
        if !number.is_finite() || number.is_sign_negative() {
            None
        } else {
            Some(number.round() as u64)
        }
    })
}

fn localized_display_name(
    localized_attributes: Option<&HashMap<String, RawLocalizedAttribute>>,
) -> Option<&str> {
    preferred_localized_value(localized_attributes, |item| item.display_name.as_deref())
}

fn localized_description(
    localized_attributes: Option<&HashMap<String, RawLocalizedAttribute>>,
) -> Option<&str> {
    preferred_localized_value(localized_attributes, |item| item.description.as_deref())
}

fn preferred_localized_value<'a>(
    localized_attributes: Option<&'a HashMap<String, RawLocalizedAttribute>>,
    field: impl Fn(&'a RawLocalizedAttribute) -> Option<&'a str>,
) -> Option<&'a str> {
    let localized_attributes = localized_attributes?;

    ["en-us", "id-id"]
        .into_iter()
        .filter_map(|locale| localized_attributes.get(locale).and_then(&field))
        .find(|value| !value.trim().is_empty())
        .or_else(|| {
            localized_attributes
                .values()
                .filter_map(field)
                .find(|value| !value.trim().is_empty())
        })
}

fn first_non_empty<'a>(candidates: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn insight_summary_line(item: &RawInsightItem) -> Option<String> {
    let statement = first_non_empty([
        item.short_insight_statement.as_deref(),
        item.insight_statement.as_deref(),
    ])?;

    let name = first_non_empty([item.insight_name.as_deref()]);
    Some(match name {
        Some(name) => format!("{name}: {statement}"),
        None => statement.to_string(),
    })
}

fn insight_status(item: &RawInsightItem) -> Option<&str> {
    item.details
        .as_ref()
        .and_then(|details| details.evaluation_status.as_deref())
        .or_else(|| {
            item.category
                .as_deref()
                .filter(|category| category.eq_ignore_ascii_case("risk"))
                .map(|_| "bad")
        })
        .and_then(|status| {
            let normalized = status.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
}

fn insight_overview(
    display_name: Option<&str>,
    positive: usize,
    negative: usize,
    neutral: usize,
) -> String {
    if positive == 0 && negative == 0 && neutral == 0 {
        return first_non_empty([display_name])
            .unwrap_or_default()
            .to_string();
    }

    let tone = if positive > 0 && negative > 0 {
        "Mixed analyst signals"
    } else if negative > 0 {
        "Mostly negative analyst signals"
    } else if positive > 0 {
        "Mostly positive analyst signals"
    } else {
        "Neutral analyst signals"
    };

    let target = first_non_empty([display_name])
        .map(|name| format!(" for {name}"))
        .unwrap_or_default();
    format!("{tone}{target}: {positive} positive, {negative} negative, {neutral} neutral.")
}

fn normalize_timestamp(value: Option<&str>) -> Option<String> {
    let value = first_non_empty([value])?;
    if value.starts_with("0001-01-01") {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn parse_profile(symbol: &str, raw: &[RawEquity]) -> Result<CompanyProfile, IdxError> {
    let equity = raw
        .first()
        .ok_or_else(|| IdxError::ParseError("no profile data".into()))?;
    let company = equity.company.as_ref();
    let company_address = company.and_then(|item| item.address.as_ref());

    Ok(CompanyProfile {
        id: equity.id.clone().unwrap_or_default(),
        symbol: equity.symbol.clone().unwrap_or_else(|| symbol.to_string()),
        short_name: equity.short_name.clone().unwrap_or_default(),
        long_name: first_non_empty([
            localized_display_name(equity.localized_attributes.as_ref()),
            equity.display_name.as_deref(),
            equity.long_name.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        description: first_non_empty([
            company.and_then(|item| item.description.as_deref()),
            localized_description(equity.localized_attributes.as_ref()),
            equity.description.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        sector: first_non_empty([
            company.and_then(|item| item.sector.as_deref()),
            equity.sector.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        industry: first_non_empty([
            company.and_then(|item| item.industry.as_deref()),
            equity.industry.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        website: first_non_empty([
            company.and_then(|item| item.website.as_deref()),
            equity.website.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        employees: company
            .and_then(|item| item.employees)
            .or(equity.full_time_employees)
            .unwrap_or_default(),
        address: first_non_empty([
            company_address.and_then(|item| item.street.as_deref()),
            equity.address.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        city: first_non_empty([
            company_address.and_then(|item| item.city.as_deref()),
            equity.city.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        country: first_non_empty([
            company_address.and_then(|item| item.country.as_deref()),
            equity.country.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        phone: first_non_empty([
            company_address.and_then(|item| item.phone.as_deref()),
            equity.phone.as_deref(),
        ])
        .unwrap_or_default()
        .to_string(),
        officers: equity
            .officers
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .map(|officer| Officer {
                        name: officer.name.clone().unwrap_or_default(),
                        title: officer.title.clone().unwrap_or_default(),
                        age: officer.age,
                        year_born: officer.year_born,
                        total_pay: officer.total_pay,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub(super) fn parse_financial_statements(
    symbol: &str,
    raw: &[RawFinancialStatement],
) -> Result<FinancialStatements, IdxError> {
    let item = raw
        .first()
        .ok_or_else(|| IdxError::ParseError("no financial statements".into()))?;
    let instrument = item.underlying_instrument.as_ref();
    Ok(FinancialStatements {
        instrument: InstrumentInfo {
            id: instrument
                .and_then(|v| v.instrument_id.clone())
                .unwrap_or_default(),
            symbol: instrument
                .and_then(|v| v.symbol.as_deref().and_then(ticker_from_symbol))
                .map(|ticker| normalized_symbol(symbol, &ticker))
                .unwrap_or_else(|| symbol.to_string()),
            name: instrument
                .and_then(|v| v.display_name.clone().or_else(|| v.short_name.clone()))
                .unwrap_or_default(),
        },
        balance_sheet: item.balance_sheets.as_ref().map(parse_statement_section),
        cash_flow: item.cash_flow.as_ref().map(parse_statement_section),
        income_statement: item.income_statements.as_ref().map(parse_statement_section),
    })
}

pub(super) fn parse_earnings(
    symbol: &str,
    raw: &RawEarningsResponse,
) -> Result<EarningsReport, IdxError> {
    let mut forecast = Vec::new();
    let mut history = Vec::new();

    if let Some(bucket) = &raw.forecast {
        collect_earnings(bucket.annual.as_ref(), &mut forecast);
        collect_earnings(bucket.quarterly.as_ref(), &mut forecast);
    }
    if let Some(bucket) = &raw.history {
        collect_earnings(bucket.annual.as_ref(), &mut history);
        collect_earnings(bucket.quarterly.as_ref(), &mut history);
    }

    forecast.sort_by_key(|row| row.earning_release_date.clone().unwrap_or_default());
    history.sort_by_key(|row| row.earning_release_date.clone().unwrap_or_default());

    Ok(EarningsReport {
        symbol: symbol.to_string(),
        eps_last_year: raw.eps_last_year.unwrap_or_default(),
        revenue_last_year: raw.revenue_last_year.unwrap_or_default(),
        forecast,
        history,
    })
}

pub(super) fn parse_sentiment(
    symbol: &str,
    raw: &[RawSentiment],
) -> Result<SentimentData, IdxError> {
    // MSN answers `[]` for tickers nobody has voted on (all IDX names as of
    // 2026-10). That is "no votes", not a malformed response.
    let Some(item) = raw.first() else {
        let ticker = ticker_from_symbol(symbol).unwrap_or_default();
        return Ok(SentimentData {
            symbol: normalized_symbol(symbol, &ticker),
            statistics: Vec::new(),
        });
    };
    let stats = item
        .sentiment_statistics
        .as_ref()
        .map(|items| {
            items
                .iter()
                .map(|it| SentimentPeriod {
                    time_range: it.time_range_name.clone().unwrap_or_default(),
                    bullish: it.bullish.unwrap_or_default(),
                    bearish: it.bearish.unwrap_or_default(),
                    neutral: it.neutral.unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(SentimentData {
        symbol: item.symbol.clone().unwrap_or_else(|| symbol.to_string()),
        statistics: stats,
    })
}

pub(super) fn parse_insights(symbol: &str, raw: &[RawInsight]) -> Result<InsightData, IdxError> {
    let item = raw
        .first()
        .ok_or_else(|| IdxError::ParseError("no insights data".into()))?;

    let insights = item.insights.as_deref().unwrap_or(&[]);
    let mut positive = 0usize;
    let mut negative = 0usize;
    let mut neutral = 0usize;
    let mut highlights = Vec::new();
    let mut risks = Vec::new();

    for insight in insights {
        let Some(statement) = insight_summary_line(insight) else {
            continue;
        };

        match insight_status(insight) {
            Some("bad") => {
                negative += 1;
                risks.push(statement);
            }
            Some("good") => {
                positive += 1;
                highlights.push(statement);
            }
            _ => {
                neutral += 1;
                highlights.push(statement);
            }
        }
    }

    Ok(InsightData {
        id: item
            .instrument_id
            .clone()
            .unwrap_or_else(|| symbol.to_string()),
        symbol: symbol.to_string(),
        summary: insight_overview(item.display_name.as_deref(), positive, negative, neutral),
        highlights,
        risks,
        last_updated: normalize_timestamp(item.time_last_updated.as_deref())
            .or_else(|| {
                insights
                    .iter()
                    .filter_map(|insight| normalize_timestamp(insight.time_last_updated.as_deref()))
                    .max()
            })
            .unwrap_or_default(),
    })
}

/// Parses a stock's news feed, ranks this company's items first and then the
/// rest (general market news), newest first within each, and keeps `limit`.
/// An item is about the company when MSN tags it with the stock's instrument
/// id (`msn_id`), or its title or description names the ticker or one of
/// `names`.
pub(super) fn parse_news(
    symbol: &str,
    msn_id: &str,
    names: &[String],
    raw: &RawNewsFeed,
    limit: usize,
) -> Result<Vec<NewsItem>, IdxError> {
    let source = raw
        .sub_cards
        .as_ref()
        .or(raw.value.as_ref())
        .ok_or_else(|| IdxError::ParseError("no news data".into()))?;
    let ticker = ticker_from_symbol(symbol).unwrap_or_default();

    let mut items: Vec<NewsItem> = source
        .iter()
        .map(|item| {
            let title = item.title.clone().unwrap_or_default();
            let description = item.description.clone().unwrap_or_default();
            let tagged = item.finance_metadata.as_ref().is_some_and(|meta| {
                meta.stocks
                    .iter()
                    .any(|stock| stock.stock_id.as_deref() == Some(msn_id))
            });
            let named = [&title, &description]
                .iter()
                .any(|text| mentions_company(text, &ticker, names));
            NewsItem {
                id: item.id.clone().unwrap_or_default(),
                symbol: symbol.to_string(),
                title,
                url: item.url.clone().unwrap_or_default(),
                description,
                provider: item
                    .provider
                    .as_ref()
                    .and_then(|p| p.name.clone())
                    .unwrap_or_default(),
                published_at: item.published_date_time.clone().unwrap_or_default(),
                read_time_min: item.read_time_min,
                relevance: if tagged || named {
                    NewsRelevance::Company
                } else {
                    NewsRelevance::Market
                },
            }
        })
        .collect();
    // Compare instants, not strings (fractional seconds and offsets vary);
    // missing or unparseable dates sort last.
    let instant = |item: &NewsItem| DateTime::parse_from_rfc3339(&item.published_at).ok();
    items.sort_by(|a, b| {
        a.relevance
            .cmp(&b.relevance)
            .then_with(|| instant(b).cmp(&instant(a)))
    });
    items.truncate(limit);
    Ok(items)
}

/// Whether `text` names the ticker as a whole word (tickers are written in
/// capitals) or contains one of the company `names`, ignoring case.
fn mentions_company(text: &str, ticker: &str, names: &[String]) -> bool {
    let whole_word = |at: usize| {
        let before = text[..at].chars().next_back();
        let after = text[at + ticker.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    };
    if !ticker.is_empty() && text.match_indices(ticker).any(|(at, _)| whole_word(at)) {
        return true;
    }
    let text = text.to_lowercase();
    names.iter().any(|name| text.contains(&name.to_lowercase()))
}

/// Names an article may use for a company, from MSN's quote names: the short
/// name ("Telkom Indonesia") and the display name without legal suffixes
/// ("Telkom Indonesia (Persero) Tbk PT" -> "Telkom Indonesia"). Abbreviated
/// short names ("Bank Cntrl Asia") never match and are harmless.
pub(super) fn company_names(short_name: Option<&str>, display_name: Option<&str>) -> Vec<String> {
    const SUFFIXES: [&str; 3] = ["(persero)", "tbk", "pt"];
    let display = display_name.map(|name| {
        name.split_whitespace()
            .filter(|word| !SUFFIXES.contains(&word.to_lowercase().trim_end_matches('.')))
            .collect::<Vec<_>>()
            .join(" ")
    });
    let mut names: Vec<String> = Vec::new();
    for name in [short_name.map(str::to_string), display]
        .into_iter()
        .flatten()
    {
        let name = name.trim().to_string();
        // Short fragments ("PT", "BRI") would match unrelated text.
        if name.len() >= 6 && !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names
}

pub(super) fn parse_screener_results(raw: &RawScreenerResponse) -> Result<Vec<Quote>, IdxError> {
    let quotes = raw
        .quote
        .as_ref()
        .ok_or_else(|| IdxError::ParseError("no screener data".into()))?;

    let results: Vec<Quote> = quotes
        .iter()
        .filter_map(|q| {
            let raw_price = q.price.filter(|price| price.is_finite() && *price > 0.0)?;
            let price = round_price(raw_price)?;
            let prev_close = q.price_previous_close.and_then(round_price);
            let change = prev_close
                .map(|pc| price - pc)
                .or_else(|| q.price_change.and_then(round_price));
            let ticker = q
                .symbol
                .as_deref()
                .and_then(ticker_from_symbol)
                .filter(|ticker| !ticker.is_empty())?;
            let (week52_position, range_signal) = match (q.price_52w_low, q.price_52w_high) {
                (Some(low), Some(high)) if high > low => {
                    let pos = (raw_price - low) / (high - low);
                    let sig = if pos > 0.66 {
                        "upper"
                    } else if pos < 0.33 {
                        "lower"
                    } else {
                        "middle"
                    };
                    (Some(pos), Some(sig.to_string()))
                }
                _ => (None, None),
            };
            Some(Quote {
                symbol: normalized_symbol(&ticker, &ticker),
                price,
                change,
                change_pct: q.price_change_percent,
                volume: round_u64(q.accumulated_volume),
                market_cap: round_u64(q.market_cap),
                week52_high: q.price_52w_high.and_then(round_price),
                week52_low: q.price_52w_low.and_then(round_price),
                week52_position,
                range_signal,
                prev_close,
                avg_volume: round_u64(q.average_volume),
                as_of: crate::api::parse_as_of(q.time_last_traded.as_deref()),
            })
        })
        .collect();

    if results.is_empty() {
        return Err(IdxError::ParseError(
            "screener returned no priced stocks".into(),
        ));
    }
    Ok(results)
}

pub(super) fn parse_history(
    symbol: &str,
    charts: &[RawChartResponse],
) -> Result<Vec<Ohlc>, IdxError> {
    let chart = charts.first().ok_or(IdxError::ProviderUnavailable)?;
    let series = chart.series.as_ref().ok_or(IdxError::ProviderUnavailable)?;
    let mut out = Vec::new();

    for (idx, raw_ts) in series.time_stamps.iter().enumerate() {
        let Some(close_raw) = series.prices.get(idx).copied().flatten() else {
            continue;
        };
        let Some(close) = round_price(close_raw) else {
            continue;
        };
        let timestamp = DateTime::parse_from_rfc3339(raw_ts)
            .map_err(|e| IdxError::ParseError(format!("msn chart timestamp '{raw_ts}': {e}")))?;
        let open = series_price_at(&series.open_prices, idx).unwrap_or(close);
        let high = series_price_at(&series.prices_high, idx).unwrap_or(close);
        let low = series_price_at(&series.prices_low, idx).unwrap_or(close);
        let volume = series_volume_at(series, idx);

        out.push(Ohlc {
            // MSN stamps each IDX day at local midnight (17:00Z the day before).
            date: timestamp
                .with_timezone(&crate::api::idx_offset())
                .date_naive(),
            open,
            high,
            low,
            close,
            volume,
        });
    }

    if out.is_empty() {
        return Err(IdxError::ProviderUnavailable);
    }

    if let Some(raw_symbol) = chart.symbol.as_deref()
        && let (Some(expected), Some(actual)) =
            (ticker_from_symbol(symbol), ticker_from_symbol(raw_symbol))
        && expected != actual
    {
        return Err(IdxError::SymbolNotFound(symbol.to_string()));
    }

    Ok(out)
}

fn series_price_at(values: &[Option<f64>], idx: usize) -> Option<i64> {
    values
        .get(idx)
        .copied()
        .flatten()
        .filter(|value| value.is_finite())
        .and_then(round_price)
}

fn series_volume_at(series: &RawChartSeries, idx: usize) -> u64 {
    series
        .volumes
        .get(idx)
        .copied()
        .flatten()
        .filter(|value| value.is_finite() && !value.is_sign_negative())
        .map(|value| value.round() as u64)
        .unwrap_or(0)
}

fn parse_statement_section(section: &RawStatementSection) -> StatementSection {
    // MSN financial statement values are nested one level deep inside sub-objects
    // (e.g., incomeStatement.income.{lineItems}, incomeStatement.revenue.{lineItems})
    // Flatten all numeric values from any depth-1 sub-object into a single map.
    let skip_keys = [
        "currency",
        "source",
        "sourceDate",
        "reportDate",
        "endDate",
        "fiscalYearEndMonth",
        "statementType",
        "type",
        "_p",
        "_t",
        "year",
        "underlyingInstrument",
        "id",
    ];
    let mut values = std::collections::BTreeMap::new();

    for (k, v) in &section.data {
        if skip_keys.contains(&k.as_str()) {
            continue;
        }
        if let Some(num) = v.as_f64() {
            // Direct numeric value at top level
            values.insert(k.to_string(), num);
        } else if let Some(obj) = v.as_object() {
            // Nested sub-object — flatten one level (e.g., income.{lineItem: value})
            for (sub_k, sub_v) in obj {
                if let Some(num) = sub_v.as_f64() {
                    values.insert(sub_k.to_string(), num);
                }
            }
        }
    }

    StatementSection {
        values,
        currency: section.currency.clone().unwrap_or_default(),
        report_date: section.report_date.clone().unwrap_or_default(),
        end_date: section.end_date.clone().unwrap_or_default(),
    }
}

fn collect_earnings(
    values: Option<&std::collections::HashMap<String, RawEarningsData>>,
    out: &mut Vec<EarningsData>,
) {
    let Some(values) = values else {
        return;
    };
    let mut rows: Vec<(&String, &RawEarningsData)> = values.iter().collect();
    rows.sort_by_key(|(k, _)| (*k).clone());
    for (key, v) in rows {
        out.push(EarningsData {
            eps_actual: v.eps_actual,
            eps_forecast: v.eps_forecast,
            eps_surprise: v.eps_surprise,
            eps_surprise_pct: v.eps_surprise_percent,
            revenue_actual: v.revenue_actual,
            revenue_forecast: v.revenue_forecast,
            revenue_surprise: v.revenue_surprise,
            earning_release_date: v.earning_release_date.clone(),
            period_type: v
                .ciq_fiscal_period_type
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| key.to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        KeyRatios, MsnQuote, NewsRelevance, RawChartResponse, RawFinancialStatement, RawNewsFeed,
        RawScreenerResponse, RawSentiment, company_names, parse_financial_statements,
        parse_fundamentals, parse_history, parse_news, parse_quote, parse_screener_results,
        parse_sentiment,
    };
    use crate::error::IdxError;

    #[test]
    fn parses_sentiment_fixture_statistics() {
        let raw: Vec<RawSentiment> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/msn_sentiment_bbca.json"
        ))
        .expect("sentiment fixture should deserialize");

        let sentiment = parse_sentiment("BBCA", &raw).expect("sentiment should parse");

        assert_eq!(sentiment.symbol, "BBCA.JK");
        assert_eq!(sentiment.statistics.len(), 2);
        assert_eq!(sentiment.statistics[0].time_range, "1D");
        assert_eq!(sentiment.statistics[0].bullish, 10);
        assert_eq!(sentiment.statistics[0].bearish, 2);
        assert_eq!(sentiment.statistics[0].neutral, 3);
    }

    #[test]
    fn parse_quote_rejects_implausible_prices_instead_of_overflowing() {
        let raw: Vec<MsnQuote> =
            serde_json::from_str(r#"[{"symbol":"BBCA","price":1e20,"pricePreviousClose":-1e20}]"#)
                .unwrap();
        assert!(parse_quote("BBCA.JK", &raw).is_err());

        let raw: Vec<MsnQuote> = serde_json::from_str(
            r#"[{"symbol":"BBCA","price":6075.0,"pricePreviousClose":-1e20}]"#,
        )
        .unwrap();
        let quote = parse_quote("BBCA.JK", &raw).unwrap();
        assert_eq!(quote.price, 6075);
        assert_eq!(quote.prev_close, None);
    }

    #[test]
    fn parses_news_fixture_items() {
        let raw: RawNewsFeed =
            serde_json::from_str(include_str!("../../../tests/fixtures/msn_news_bbca.json"))
                .expect("news fixture should deserialize");

        let items = parse_news("BBCA.JK", "bn91jc", &[], &raw, 10).expect("news should parse");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "news-1");
        assert_eq!(items[0].symbol, "BBCA.JK");
        assert_eq!(items[0].title, "BCA reports steady growth");
        assert_eq!(items[0].provider, "Contoso News");
        assert_eq!(items[0].published_at, "2026-03-20T10:00:00Z");
        assert_eq!(items[0].read_time_min, Some(3));
        assert_eq!(items[0].relevance, NewsRelevance::Market);
    }

    fn news_feed(items: serde_json::Value) -> RawNewsFeed {
        serde_json::from_value(serde_json::json!({ "value": items })).expect("news feed")
    }

    #[test]
    fn parse_news_ranks_company_items_first_then_newest() {
        let raw = news_feed(serde_json::json!([
            {"id": "market-new", "title": "IHSG anjlok usai pengumuman MSCI",
             "publishedDateTime": "2026-10-16T08:00:00Z",
             "financeMetadata": {"stocks": [{"stockId": "a1xz1h"}]}},
            {"id": "tagged", "title": "Laba bersih naik",
             "publishedDateTime": "2026-10-14T08:00:00Z",
             "financeMetadata": {"stocks": [{"stockId": "bn9o8m"}]}},
            {"id": "ticker", "title": "Saham TLKM menguat",
             "publishedDateTime": "2026-10-15T08:00:00Z"},
            {"id": "name", "title": "Dirut baru", "abstract": "Telkom Indonesia menunjuk direksi",
             "publishedDateTime": "2026-10-13T08:00:00Z"},
            {"id": "market-old", "title": "Rupiah melemah",
             "publishedDateTime": "2026-10-12T08:00:00Z"}
        ]));
        let names = vec!["Telkom Indonesia".to_string()];

        let items = parse_news("TLKM.JK", "bn9o8m", &names, &raw, 10).expect("news");

        let order: Vec<_> = items.iter().map(|i| (i.id.as_str(), i.relevance)).collect();
        assert_eq!(
            order,
            [
                ("ticker", NewsRelevance::Company),
                ("tagged", NewsRelevance::Company),
                ("name", NewsRelevance::Company),
                ("market-new", NewsRelevance::Market),
                ("market-old", NewsRelevance::Market),
            ]
        );
        let limited = parse_news("TLKM.JK", "bn9o8m", &names, &raw, 2).expect("news");
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[1].id, "tagged");
    }

    #[test]
    fn parse_news_orders_by_instant_with_undated_items_last() {
        let raw = news_feed(serde_json::json!([
            {"id": "undated", "title": "No date"},
            {"id": "whole", "title": "A", "publishedDateTime": "2026-10-16T10:00:00Z"},
            {"id": "fraction", "title": "B", "publishedDateTime": "2026-10-16T10:00:00.500Z"},
            {"id": "offset", "title": "C", "publishedDateTime": "2026-10-16T16:30:00+07:00"},
            {"id": "garbage", "title": "D", "publishedDateTime": "yesterday"}
        ]));

        let items = parse_news("BBCA.JK", "bn91jc", &[], &raw, 10).expect("news");

        let order: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
        // 16:30+07:00 is 09:30Z, the oldest dated item.
        assert_eq!(&order[..3], ["fraction", "whole", "offset"]);
        assert!(order[3..].contains(&"undated") && order[3..].contains(&"garbage"));
    }

    #[test]
    fn ticker_match_requires_a_whole_word() {
        let raw = news_feed(serde_json::json!([
            {"id": "a", "title": "BBRI dan BMRI melemah"},
            {"id": "b", "title": "Saham XBBRI dan BBRIX"},
            {"id": "c", "title": "bbri lowercase is not a ticker"}
        ]));

        let items = parse_news("BBRI.JK", "bn91p2", &[], &raw, 10).expect("news");

        let company: Vec<_> = items
            .iter()
            .filter(|i| i.relevance == NewsRelevance::Company)
            .map(|i| i.id.as_str())
            .collect();
        assert_eq!(company, ["a"]);
    }

    #[test]
    fn company_names_strip_legal_suffixes_and_skip_short_names() {
        assert_eq!(
            company_names(
                Some("Telkom Indonesia"),
                Some("Telkom Indonesia (Persero) Tbk PT")
            ),
            ["Telkom Indonesia"]
        );
        assert_eq!(
            company_names(
                Some("Bank BRI"),
                Some("Bank Rakyat Indonesia (Persero) Tbk PT")
            ),
            ["Bank BRI", "Bank Rakyat Indonesia"]
        );
        assert!(company_names(Some("PT"), Some("Tbk PT")).is_empty());
    }

    #[test]
    fn parse_financial_statements_normalizes_instrument_symbol() {
        let raw: Vec<RawFinancialStatement> = serde_json::from_str(
            r#"[
                {
                    "underlyingInstrument": {
                        "instrumentId": "bn91jc",
                        "displayName": "Bank Central Asia Tbk PT",
                        "symbol": "BBCA"
                    },
                    "balanceSheets": {
                        "currency": "IDR",
                        "reportDate": "2025-03-31T00:00:00Z",
                        "endDate": "2025-03-31T00:00:00Z",
                        "totalAssets": 1533763445000000.0
                    }
                }
            ]"#,
        )
        .expect("financial statements should deserialize");

        let financials =
            parse_financial_statements("BBCA.JK", &raw).expect("financials should parse");

        assert_eq!(financials.instrument.id, "bn91jc");
        assert_eq!(financials.instrument.symbol, "BBCA.JK");
        assert_eq!(financials.instrument.name, "Bank Central Asia Tbk PT");
    }

    #[test]
    fn parses_screener_fixture_quotes() {
        let raw: RawScreenerResponse = serde_json::from_str(include_str!(
            "../../../tests/fixtures/msn_screener_id_topperfs.json"
        ))
        .expect("screener fixture should deserialize");

        let quotes = parse_screener_results(&raw).expect("screener should parse");

        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0].symbol, "BBCA.JK");
        assert_eq!(quotes[0].price, 9_875);
        assert_eq!(quotes[0].change, Some(117));
        assert_eq!(quotes[0].market_cap, Some(1_215_200_000_000_000));
        assert_eq!(quotes[0].range_signal.as_deref(), Some("upper"));
        assert_eq!(quotes[0].avg_volume, Some(10_000_000));
    }

    #[test]
    fn parse_screener_results_filters_invalid_rows() {
        let raw: RawScreenerResponse = serde_json::from_str(
            r#"{
                "quote": [
                    { "symbol": "BBCA", "price": 9875, "pricePreviousClose": 9758 },
                    { "symbol": "BBRI", "price": 0 },
                    { "symbol": "BMRI" },
                    { "symbol": "", "price": 5150 }
                ]
            }"#,
        )
        .expect("screener fixture should deserialize");

        let quotes = parse_screener_results(&raw).expect("screener should parse");

        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "BBCA.JK");
        assert_eq!(quotes[0].price, 9_875);
    }

    #[test]
    fn parse_screener_results_errors_when_all_rows_are_invalid() {
        let raw: RawScreenerResponse = serde_json::from_str(
            r#"{
                "quote": [
                    { "symbol": "BBRI", "price": 0 },
                    { "symbol": "BMRI" }
                ]
            }"#,
        )
        .expect("screener fixture should deserialize");

        let err = parse_screener_results(&raw).expect_err("invalid screener rows should fail");

        assert!(matches!(err, IdxError::ParseError(_)));
        assert_eq!(
            err.to_string(),
            "parse error: screener returned no priced stocks"
        );
    }

    #[test]
    fn parse_fundamentals_rejects_industry_only_metrics() {
        let raw: Vec<KeyRatios> = serde_json::from_str(
            r#"[
                {
                    "industryMetrics": [
                        {
                            "year": "2025",
                            "fiscalPeriodType": "TTM",
                            "priceToEarningsRatio": 12.5,
                            "priceToBookRatio": 1.7
                        }
                    ],
                    "companyMetrics": []
                }
            ]"#,
        )
        .expect("key ratios should deserialize");

        let err = parse_fundamentals(&raw, None).expect_err("industry fallback should be rejected");

        assert!(matches!(err, IdxError::Unsupported(_)));
        assert_eq!(
            err.to_string(),
            "unsupported: company fundamentals unavailable from MSN; industry fallback is disabled"
        );
    }

    #[test]
    fn quote_as_of_is_last_trade_in_wib() {
        let raw: Vec<MsnQuote> = serde_json::from_str(
            r#"[{"symbol":"BBCA","price":6100.0,"timeLastTraded":"2026-10-02T09:15:00Z"}]"#,
        )
        .expect("quote should deserialize");
        let quote = parse_quote("BBCA.JK", &raw).expect("quote parsed");
        assert_eq!(
            quote.as_of.unwrap().to_rfc3339(),
            "2026-10-02T16:15:00+07:00"
        );
    }

    #[test]
    fn missing_change_and_volume_stay_missing() {
        let raw: Vec<MsnQuote> = serde_json::from_str(r#"[{"symbol":"BBCA","price":6100.0}]"#)
            .expect("quote should deserialize");
        let quote = parse_quote("BBCA.JK", &raw).expect("quote parsed");
        assert_eq!(quote.change, None);
        assert_eq!(quote.change_pct, None);
        assert_eq!(quote.volume, None);
        let json = serde_json::to_value(&quote).unwrap();
        assert!(json["change_pct"].is_null() && json["volume"].is_null());
    }

    #[test]
    fn listed_quote_without_price_is_no_market_data() {
        let raw: Vec<MsnQuote> = serde_json::from_str(
            r#"[{"symbol":"WIKA","shortName":"Wijaya Karya","marketCap":8080746000000.0}]"#,
        )
        .expect("quote should deserialize");
        let err = parse_quote("WIKA.JK", &raw).unwrap_err();
        assert!(matches!(err, IdxError::NoMarketData(_)), "{err:?}");

        let raw: Vec<MsnQuote> = serde_json::from_str("[{}]").expect("quote should deserialize");
        let err = parse_quote("ZZZZ.JK", &raw).unwrap_err();
        assert!(matches!(err, IdxError::SymbolNotFound(_)), "{err:?}");
    }

    #[test]
    fn empty_sentiment_response_is_empty_data_not_an_error() {
        let sentiment = parse_sentiment("BBCA", &[]).expect("empty sentiment is valid");
        assert_eq!(sentiment.symbol, "BBCA.JK");
        assert!(sentiment.statistics.is_empty());
    }

    #[test]
    fn parses_msn_chart_price_only_fixture_as_synthetic_ohlc() {
        let raw: Vec<RawChartResponse> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/msn_chart_bbca_3m.json"
        ))
        .expect("chart fixture should deserialize");
        let history = parse_history("BBCA.JK", &raw).expect("chart history should parse");

        assert_eq!(history.len(), 3);
        // `2026-01-13T17:00:00Z` is midnight WIB on the 14th, the trading day it belongs to.
        assert_eq!(history[0].date.to_string(), "2026-01-14");
        assert_eq!(history[0].open, 8000);
        assert_eq!(history[0].high, 8000);
        assert_eq!(history[0].low, 8000);
        assert_eq!(history[0].close, 8000);
        assert_eq!(history[0].volume, 0);
    }
}
