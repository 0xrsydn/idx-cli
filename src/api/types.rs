use chrono::NaiveDate;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub price: f64,
    pub change: f64,
    pub change_pct: f64,
    pub volume: u64,
    pub market_cap: Option<f64>,
    pub week52_high: Option<f64>,
    pub week52_low: Option<f64>,
    pub week52_position: Option<f64>,
    pub range_signal: Option<String>,
    pub prev_close: Option<f64>,
    pub avg_volume: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ohlc {
    pub date: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
pub enum Period {
    #[value(name = "1d")]
    OneDay,
    #[value(name = "5d")]
    FiveDays,
    #[value(name = "1mo")]
    OneMonth,
    #[value(name = "3mo")]
    ThreeMonths,
    #[value(name = "6mo")]
    SixMonths,
    #[value(name = "1y")]
    OneYear,
    #[value(name = "2y")]
    TwoYears,
    #[value(name = "5y")]
    FiveYears,
}

impl Period {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OneDay => "1d",
            Self::FiveDays => "5d",
            Self::OneMonth => "1mo",
            Self::ThreeMonths => "3mo",
            Self::SixMonths => "6mo",
            Self::OneYear => "1y",
            Self::TwoYears => "2y",
            Self::FiveYears => "5y",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
pub enum Interval {
    #[value(name = "1d")]
    Day,
    #[value(name = "1wk")]
    Week,
    #[value(name = "1mo")]
    Month,
}

impl Interval {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "1wk",
            Self::Month => "1mo",
        }
    }
}
