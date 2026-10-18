//! Pure parser for IDX's `GetStockSummary` JSON, tested against
//! `tests/fixtures/idx_stocksummary_*.json`.

use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::IdxError;
use crate::flow::types::StockDay;

#[derive(Deserialize)]
struct StockSummaryResponse {
    data: Vec<StockSummaryRawRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StockSummaryRawRow {
    date: String,
    stock_code: String,
    previous: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    volume: Option<f64>,
    value: Option<f64>,
    frequency: Option<f64>,
    foreign_buy: Option<f64>,
    foreign_sell: Option<f64>,
    non_regular_volume: Option<f64>,
    non_regular_value: Option<f64>,
}

fn whole(value: Option<f64>) -> Option<i64> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.round() as i64)
}

/// The rows of one session. A day without trading (weekend, holiday) is
/// empty; rows dated other than `date` are dropped, so another session's
/// figures never pass as this one's.
pub fn parse_stock_summary(raw: &str, date: NaiveDate) -> Result<Vec<StockDay>, IdxError> {
    let response: StockSummaryResponse = serde_json::from_str(raw)
        .map_err(|e| IdxError::ParseError(format!("idx stock summary: {e}")))?;
    Ok(response
        .data
        .into_iter()
        .filter(|row| {
            row.date
                .get(..10)
                .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
                == Some(date)
        })
        .map(|row| StockDay {
            code: row.stock_code.trim().to_ascii_uppercase(),
            date,
            previous: whole(row.previous),
            high: whole(row.high),
            low: whole(row.low),
            close: whole(row.close),
            volume: whole(row.volume),
            value: whole(row.value),
            frequency: whole(row.frequency),
            foreign_buy: whole(row.foreign_buy),
            foreign_sell: whole(row.foreign_sell),
            non_regular_volume: whole(row.non_regular_volume),
            non_regular_value: whole(row.non_regular_value),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: &str = include_str!("../../../tests/fixtures/idx_stocksummary_20261016.json");
    const SATURDAY: &str = include_str!("../../../tests/fixtures/idx_stocksummary_20261017.json");

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_parse_stock_summary_reads_foreign_and_non_regular_figures() {
        let rows = parse_stock_summary(DAY, date(2026, 10, 16)).unwrap();
        let bbca = rows.iter().find(|row| row.code == "BBCA").unwrap();
        assert_eq!((bbca.previous, bbca.close), (Some(6075), Some(5175)));
        assert_eq!(bbca.volume, Some(1_241_098_200));
        assert_eq!(bbca.value, Some(6_474_934_315_000));
        assert_eq!(
            (bbca.foreign_buy, bbca.foreign_sell),
            (Some(309_392_800), Some(757_227_900))
        );
        assert_eq!(bbca.non_regular_value, Some(100_183_151_475));
    }

    #[test]
    fn test_parse_stock_summary_keeps_only_the_requested_session() {
        assert!(
            parse_stock_summary(SATURDAY, date(2026, 10, 17))
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_stock_summary(DAY, date(2026, 10, 15))
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            parse_stock_summary("<html>Just a moment...</html>", date(2026, 10, 16)),
            Err(IdxError::ParseError(_))
        ));
    }
}
