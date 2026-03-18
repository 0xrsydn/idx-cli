use super::raw_types::{
    IndustryMetric, KeyRatios, MsnQuote, RawEarningsData, RawEarningsResponse, RawEquity,
    RawFinancialStatement, RawInsight, RawNewsFeed, RawScreenerResponse, RawSentiment,
    RawStatementSection,
};
use super::symbols::{normalized_symbol, ticker_from_symbol};
use crate::api::types::{
    CompanyProfile, EarningsData, EarningsReport, FinancialStatements, Fundamentals, InsightData,
    InstrumentInfo, NewsItem, Officer, Quote, SentimentData, SentimentPeriod, StatementSection,
};
use crate::error::IdxError;

pub(super) fn parse_quote(symbol: &str, quotes: &[MsnQuote]) -> Result<Quote, IdxError> {
    let quote = quotes.first().ok_or(IdxError::ProviderUnavailable)?;
    let raw_price = quote
        .price
        .ok_or_else(|| IdxError::SymbolNotFound(symbol.to_string()))?;
    let prev_close = quote.price_previous_close.map(round_price);
    let price = round_price(raw_price);
    let change = prev_close
        .map(|previous| price - previous)
        .or_else(|| quote.price_change.map(round_price))
        .unwrap_or(0);

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
        change_pct: quote.price_change_percent.unwrap_or(0.0),
        volume: round_u64(quote.accumulated_volume).unwrap_or(0),
        market_cap: round_u64(quote.market_cap),
        week52_high: quote.price_52w_high.map(round_price),
        week52_low: quote.price_52w_low.map(round_price),
        week52_position,
        range_signal,
        prev_close,
        avg_volume: round_u64(quote.average_volume),
    })
}

pub(super) fn parse_fundamentals(
    ratios: &[KeyRatios],
    quote: Option<&MsnQuote>,
) -> Result<Fundamentals, IdxError> {
    let ratios = ratios.first().ok_or(IdxError::ProviderUnavailable)?;
    let metrics = if ratios.company_metrics.is_empty() {
        &ratios.industry_metrics
    } else {
        &ratios.company_metrics
    };
    if preferred_metric(metrics).is_none() {
        return Err(IdxError::ProviderUnavailable);
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

fn preferred_metric(metrics: &[IndustryMetric]) -> Option<&IndustryMetric> {
    metrics.iter().max_by_key(|metric| metric_rank(metric))
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
    value.and_then(|number| {
        if !number.is_finite() || number < 0.01 {
            None
        } else {
            Some(number)
        }
    })
}

fn round_price(value: f64) -> i64 {
    value.round() as i64
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

pub(super) fn parse_profile(symbol: &str, raw: &[RawEquity]) -> Result<CompanyProfile, IdxError> {
    let equity = raw
        .first()
        .ok_or_else(|| IdxError::ParseError("no profile data".into()))?;
    Ok(CompanyProfile {
        id: equity.id.clone().unwrap_or_default(),
        symbol: equity.symbol.clone().unwrap_or_else(|| symbol.to_string()),
        short_name: equity.short_name.clone().unwrap_or_default(),
        long_name: equity.long_name.clone().unwrap_or_default(),
        description: equity.description.clone().unwrap_or_default(),
        sector: equity.sector.clone().unwrap_or_default(),
        industry: equity.industry.clone().unwrap_or_default(),
        website: equity.website.clone().unwrap_or_default(),
        employees: equity.full_time_employees.unwrap_or_default(),
        address: equity.address.clone().unwrap_or_default(),
        city: equity.city.clone().unwrap_or_default(),
        country: equity.country.clone().unwrap_or_default(),
        phone: equity.phone.clone().unwrap_or_default(),
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
                .and_then(|v| v.symbol.clone())
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
    _symbol: &str,
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
    let item = raw
        .first()
        .ok_or_else(|| IdxError::ParseError("no sentiment data".into()))?;
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

    // Group insight statements into highlights (non-risk) and risks by category
    let highlights: Vec<String> = insights
        .iter()
        .filter(|i| {
            i.category
                .as_deref()
                .map(|c| !c.eq_ignore_ascii_case("risk"))
                .unwrap_or(true)
        })
        .filter_map(|i| {
            let name = i.insight_name.as_deref().unwrap_or("");
            let stmt = i.insight_statement.as_deref().unwrap_or("");
            if stmt.is_empty() {
                None
            } else if name.is_empty() {
                Some(stmt.to_string())
            } else {
                Some(format!("{name}: {stmt}"))
            }
        })
        .collect();

    let risks: Vec<String> = insights
        .iter()
        .filter(|i| {
            i.category
                .as_deref()
                .map(|c| c.eq_ignore_ascii_case("risk"))
                .unwrap_or(false)
        })
        .filter_map(|i| {
            let stmt = i.insight_statement.as_deref().unwrap_or("");
            if stmt.is_empty() {
                None
            } else {
                Some(stmt.to_string())
            }
        })
        .collect();

    Ok(InsightData {
        id: item
            .instrument_id
            .clone()
            .unwrap_or_else(|| symbol.to_string()),
        summary: item.display_name.clone().unwrap_or_default(),
        highlights,
        risks,
        last_updated: String::new(),
    })
}

pub(super) fn parse_news(raw: &RawNewsFeed) -> Result<Vec<NewsItem>, IdxError> {
    let source = raw
        .sub_cards
        .as_ref()
        .or(raw.value.as_ref())
        .ok_or_else(|| IdxError::ParseError("no news data".into()))?;

    Ok(source
        .iter()
        .map(|item| NewsItem {
            id: item.id.clone().unwrap_or_default(),
            title: item.title.clone().unwrap_or_default(),
            url: item.url.clone().unwrap_or_default(),
            description: item.description.clone().unwrap_or_default(),
            provider: item
                .provider
                .as_ref()
                .and_then(|p| p.name.clone())
                .unwrap_or_default(),
            published_at: item.published_date_time.clone().unwrap_or_default(),
            read_time_min: item.read_time_min,
        })
        .collect())
}

pub(super) fn parse_screener_results(raw: &RawScreenerResponse) -> Result<Vec<Quote>, IdxError> {
    let quotes = raw
        .quote
        .as_ref()
        .ok_or_else(|| IdxError::ParseError("no screener data".into()))?;

    // Build Quote directly from screener MsnQuote data; default price to 0 if missing
    // (do not route through parse_quote which errors on missing price)
    let results: Vec<Quote> = quotes
        .iter()
        .map(|q| {
            let raw_price = q.price.unwrap_or(0.0);
            let price = round_price(raw_price);
            let prev_close = q.price_previous_close.map(round_price);
            let change = prev_close
                .map(|pc| price - pc)
                .or_else(|| q.price_change.map(round_price))
                .unwrap_or(0);
            let ticker = q
                .symbol
                .as_deref()
                .and_then(ticker_from_symbol)
                .unwrap_or_default();
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
            Quote {
                symbol: normalized_symbol(&ticker, &ticker),
                price,
                change,
                change_pct: q.price_change_percent.unwrap_or(0.0),
                volume: round_u64(q.accumulated_volume).unwrap_or(0),
                market_cap: round_u64(q.market_cap),
                week52_high: q.price_52w_high.map(round_price),
                week52_low: q.price_52w_low.map(round_price),
                week52_position,
                range_signal,
                prev_close,
                avg_volume: round_u64(q.average_volume),
            }
        })
        .collect();

    if results.is_empty() {
        return Err(IdxError::ParseError(
            "screener returned no priced stocks".into(),
        ));
    }
    Ok(results)
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
    let mut values = std::collections::HashMap::new();

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
    for (_, v) in rows {
        out.push(EarningsData {
            eps_actual: v.eps_actual,
            eps_forecast: v.eps_forecast,
            eps_surprise: v.eps_surprise,
            eps_surprise_pct: v.eps_surprise_percent,
            revenue_actual: v.revenue_actual,
            revenue_forecast: v.revenue_forecast,
            revenue_surprise: v.revenue_surprise,
            earning_release_date: v.earning_release_date.clone(),
            period_type: v.ciq_fiscal_period_type.clone().unwrap_or_default(),
        });
    }
}
