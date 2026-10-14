use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MacdResult {
    pub macd_line: Vec<Option<f64>>,
    pub signal_line: Vec<Option<f64>>,
    pub histogram: Vec<Option<f64>>,
}

pub fn sma(data: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut result = vec![None; data.len()];
    if period == 0 || period > data.len() {
        return result;
    }

    let mut window_sum: f64 = data[..period].iter().sum();
    result[period - 1] = Some(window_sum / period as f64);

    for idx in period..data.len() {
        window_sum += data[idx] - data[idx - period];
        result[idx] = Some(window_sum / period as f64);
    }

    result
}

pub fn ema(data: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut result = vec![None; data.len()];
    if period == 0 || period > data.len() {
        return result;
    }

    let multiplier = 2.0 / (period as f64 + 1.0);
    let seed = data[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = Some(seed);

    let mut prev = seed;
    for idx in period..data.len() {
        let current = ((data[idx] - prev) * multiplier) + prev;
        result[idx] = Some(current);
        prev = current;
    }

    result
}

pub fn rsi(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut result = vec![None; closes.len()];
    if period == 0 || closes.len() <= period {
        return result;
    }

    let mut gains = 0.0;
    let mut losses = 0.0;

    for idx in 1..=period {
        let change = closes[idx] - closes[idx - 1];
        if change >= 0.0 {
            gains += change;
        } else {
            losses += -change;
        }
    }

    let mut avg_gain = gains / period as f64;
    let mut avg_loss = losses / period as f64;

    result[period] = Some(rsi_from_averages(avg_gain, avg_loss));

    for idx in (period + 1)..closes.len() {
        let change = closes[idx] - closes[idx - 1];
        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };

        avg_gain = ((avg_gain * (period as f64 - 1.0)) + gain) / period as f64;
        avg_loss = ((avg_loss * (period as f64 - 1.0)) + loss) / period as f64;

        result[idx] = Some(rsi_from_averages(avg_gain, avg_loss));
    }

    result
}

fn rsi_from_averages(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return 100.0;
    }

    let rs = avg_gain / avg_loss;
    100.0 - (100.0 / (1.0 + rs))
}

pub fn macd(closes: &[f64], fast: usize, slow: usize, signal: usize) -> MacdResult {
    let len = closes.len();
    let mut macd_line = vec![None; len];
    let mut signal_line = vec![None; len];
    let mut histogram = vec![None; len];

    if len == 0 || fast == 0 || slow == 0 || signal == 0 {
        return MacdResult {
            macd_line,
            signal_line,
            histogram,
        };
    }

    let fast_ema = ema(closes, fast);
    let slow_ema = ema(closes, slow);

    for idx in 0..len {
        if let (Some(f), Some(s)) = (fast_ema[idx], slow_ema[idx]) {
            macd_line[idx] = Some(f - s);
        }
    }

    let mut signal_seed = Vec::new();
    let signal_multiplier = 2.0 / (signal as f64 + 1.0);
    let mut prev_signal = None;

    for idx in 0..len {
        if let Some(value) = macd_line[idx] {
            if prev_signal.is_none() {
                signal_seed.push(value);
                if signal_seed.len() == signal {
                    let seed = signal_seed.iter().sum::<f64>() / signal as f64;
                    signal_line[idx] = Some(seed);
                    prev_signal = Some(seed);
                    histogram[idx] = Some(value - seed);
                }
            } else if let Some(prev) = prev_signal {
                let current = ((value - prev) * signal_multiplier) + prev;
                signal_line[idx] = Some(current);
                prev_signal = Some(current);
                histogram[idx] = Some(value - current);
            }
        }
    }

    MacdResult {
        macd_line,
        signal_line,
        histogram,
    }
}

pub fn volume_ratio(volumes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || volumes.len() < period {
        return None;
    }

    let start = volumes.len() - period;
    let avg = volumes[start..].iter().sum::<f64>() / period as f64;
    if avg == 0.0 {
        return None;
    }

    volumes.last().map(|last| *last / avg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(left: f64, right: f64, eps: f64) {
        assert!((left - right).abs() <= eps, "left={left}, right={right}");
    }

    #[test]
    fn sma_returns_expected_values() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let values = sma(&data, 3);
        assert_eq!(values, vec![None, None, Some(2.0), Some(3.0), Some(4.0)]);
    }

    #[test]
    fn rsi_returns_seeded_none_and_known_value() {
        // Classic Wilder example dataset; RSI(14) first computed value ~= 70.46.
        let closes = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28,
        ];
        let period = 14;
        let values = rsi(&closes, period);

        assert_eq!(values.len(), closes.len());
        assert!(values.iter().take(period).all(Option::is_none));

        let rsi_14 = values[period].expect("expected first RSI value");
        approx_eq(rsi_14, 70.46, 0.05);
    }

    #[test]
    fn macd_shapes_are_correct() {
        let closes: Vec<f64> = (1..=60).map(|n| n as f64).collect();
        let result = macd(&closes, 12, 26, 9);

        assert_eq!(result.macd_line.len(), closes.len());

        let macd_nones = result.macd_line.iter().filter(|v| v.is_none()).count();
        let signal_nones = result.signal_line.iter().filter(|v| v.is_none()).count();
        assert!(signal_nones > macd_nones);
    }

    #[test]
    fn volume_ratio_checks() {
        let volumes = [100.0, 120.0, 130.0, 150.0];
        let ratio = volume_ratio(&volumes, 3).expect("ratio should exist");
        approx_eq(ratio, 150.0 / ((120.0 + 130.0 + 150.0) / 3.0), 1e-10);

        assert_eq!(volume_ratio(&volumes, 5), None);
    }
}
