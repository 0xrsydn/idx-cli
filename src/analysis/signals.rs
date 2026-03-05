use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Bullish,
    Bearish,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalSignal {
    pub rsi: Signal,
    pub macd: Signal,
    pub trend: Signal,
    pub overall: Signal,
}

pub fn interpret_rsi(value: f64) -> Signal {
    if value > 70.0 {
        Signal::Bearish
    } else if value < 30.0 {
        Signal::Bullish
    } else {
        Signal::Neutral
    }
}

pub fn interpret_macd(histogram: f64, prev_histogram: Option<f64>) -> Signal {
    if let Some(prev) = prev_histogram {
        if histogram > 0.0 && histogram > prev {
            Signal::Bullish
        } else if histogram < 0.0 && histogram < prev {
            Signal::Bearish
        } else {
            Signal::Neutral
        }
    } else {
        Signal::Neutral
    }
}

pub fn interpret_trend(price: f64, sma50: Option<f64>, sma200: Option<f64>) -> Signal {
    match (sma50, sma200) {
        (Some(s50), Some(s200)) if price > s50 && price > s200 => Signal::Bullish,
        (Some(s50), Some(s200)) if price < s50 && price < s200 => Signal::Bearish,
        _ => Signal::Neutral,
    }
}

pub fn overall_signal(rsi: Signal, macd: Signal, trend: Signal) -> Signal {
    let signals = [rsi, macd, trend];
    let bullish = signals.iter().filter(|&&s| s == Signal::Bullish).count();
    let bearish = signals.iter().filter(|&&s| s == Signal::Bearish).count();

    if bullish >= 2 {
        Signal::Bullish
    } else if bearish >= 2 {
        Signal::Bearish
    } else {
        Signal::Neutral
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_rsi_thresholds() {
        assert_eq!(interpret_rsi(75.0), Signal::Bearish);
        assert_eq!(interpret_rsi(25.0), Signal::Bullish);
        assert_eq!(interpret_rsi(50.0), Signal::Neutral);
    }

    #[test]
    fn interpret_trend_thresholds() {
        assert_eq!(
            interpret_trend(120.0, Some(100.0), Some(110.0)),
            Signal::Bullish
        );
        assert_eq!(
            interpret_trend(80.0, Some(100.0), Some(90.0)),
            Signal::Bearish
        );
    }

    #[test]
    fn overall_majority_vote() {
        assert_eq!(
            overall_signal(Signal::Bullish, Signal::Bullish, Signal::Neutral),
            Signal::Bullish
        );
        assert_eq!(
            overall_signal(Signal::Bearish, Signal::Neutral, Signal::Bearish),
            Signal::Bearish
        );
        assert_eq!(
            overall_signal(Signal::Bullish, Signal::Bearish, Signal::Neutral),
            Signal::Neutral
        );
    }
}
