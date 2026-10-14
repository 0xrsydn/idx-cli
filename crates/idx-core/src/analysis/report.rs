//! Technical analysis report built from daily history.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::signals::{self, Signal, TechnicalSignal};
use super::technical;
use crate::api::types::Ohlc;
use crate::error::IdxError;

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

pub fn build_technical_report(symbol: &str, history: &[Ohlc]) -> Result<TechnicalReport, IdxError> {
    let latest = history
        .last()
        .ok_or_else(|| IdxError::ParseError(format!("no history available for {symbol}")))?;
    let closes: Vec<f64> = history.iter().map(|item| item.close as f64).collect();
    let volumes: Vec<f64> = history.iter().map(|item| item.volume as f64).collect();

    let sma20 = last_value(&technical::sma(&closes, 20));
    let sma50 = last_value(&technical::sma(&closes, 50));
    let sma200 = last_value(&technical::sma(&closes, 200));
    let rsi14 = last_value(&technical::rsi(&closes, 14));
    let macd = technical::macd(&closes, 12, 26, 9);
    let macd_line = last_value(&macd.macd_line);
    let signal_line = last_value(&macd.signal_line);
    let histogram = last_value(&macd.histogram);
    let previous_histogram = previous_value(&macd.histogram);
    let average_volume20 = average_last(&volumes, 20);
    let volume_ratio20 = technical::volume_ratio(&volumes, 20);

    let rsi_signal = rsi14.map_or(Signal::Neutral, signals::interpret_rsi);
    let macd_signal = histogram
        .map(|value| signals::interpret_macd(value, previous_histogram))
        .unwrap_or(Signal::Neutral);
    let trend_signal = signals::interpret_trend(latest.close as f64, sma50, sma200);
    let overall = signals::overall_signal(rsi_signal, macd_signal, trend_signal);

    Ok(TechnicalReport {
        symbol: symbol.to_string(),
        as_of: latest.date,
        current_price: latest.close,
        sma20,
        sma50,
        sma200,
        rsi14,
        macd: MacdSnapshot {
            line: macd_line,
            signal: signal_line,
            histogram,
        },
        volume: VolumeSnapshot {
            current: latest.volume,
            average20: average_volume20,
            ratio20: volume_ratio20,
        },
        signals: TechnicalSignal {
            rsi: rsi_signal,
            macd: macd_signal,
            trend: trend_signal,
            overall,
        },
    })
}

fn last_value(values: &[Option<f64>]) -> Option<f64> {
    values.iter().rev().find_map(|value| *value)
}

fn previous_value(values: &[Option<f64>]) -> Option<f64> {
    let mut seen_latest = false;
    for value in values.iter().rev() {
        if value.is_some() {
            if seen_latest {
                return *value;
            }
            seen_latest = true;
        }
    }
    None
}

fn average_last(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }

    let start = values.len() - period;
    Some(values[start..].iter().sum::<f64>() / period as f64)
}

#[cfg(test)]
mod tests {
    use chrono::{Days, NaiveDate};

    use super::build_technical_report;
    use crate::analysis::signals::Signal;
    use crate::api::types::Ohlc;

    #[test]
    fn technical_report_uses_latest_values() {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date");
        let history: Vec<Ohlc> = (0..60)
            .map(|idx| Ohlc {
                date: start
                    .checked_add_days(Days::new(idx as u64))
                    .expect("valid offset"),
                open: 100 + idx as i64,
                high: 101 + idx as i64,
                low: 99 + idx as i64,
                close: 100 + idx as i64,
                volume: 1_000 + idx as u64 * 10,
            })
            .collect();

        let report = build_technical_report("BBCA.JK", &history).expect("report should build");

        assert_eq!(report.symbol, "BBCA.JK");
        assert_eq!(report.current_price, 159);
        assert!(report.sma20.is_some());
        assert!(report.sma50.is_some());
        assert_eq!(report.sma200, None);
        assert!(report.rsi14.is_some());
        assert!(report.volume.ratio20.is_some());
        assert_eq!(report.signals.trend, Signal::Neutral);
    }
}
