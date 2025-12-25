use crate::bars::Bar;
use crate::levels::DailyLevels;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Minimum points for a valid NQ impulse move
const MIN_IMPULSE_POINTS: f64 = 30.0;

/// Maximum candles for a "fast" move
const MAX_FAST_CANDLES: usize = 5;

/// Minimum score for valid impulse (out of 5)
const MIN_IMPULSE_SCORE: u8 = 4;

/// Swing lookback period (bars)
const SWING_LOOKBACK: usize = 10;

/// Direction of impulse move
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpulseDirection {
    Up,
    Down,
}

/// Detected impulse leg with scoring details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpulseLeg {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub start_price: f64,
    pub end_price: f64,
    pub direction: ImpulseDirection,
    pub symbol: String,
    pub date: NaiveDate,

    // Scoring breakdown (each 0 or 1)
    pub score_total: u8,
    pub broke_swing: bool,        // Did it break prior swing high/low?
    pub was_fast: bool,           // 3-5 candles max
    pub uniform_candles: bool,    // Mostly one color, little overlap
    pub volume_increased: bool,   // Volume increased on move
    pub sufficient_size: bool,    // Move >= 30 points

    // Additional metrics
    pub num_candles: usize,
    pub total_volume: u64,
    pub avg_volume_per_bar: u64,
}

/// Detect impulse legs from 1-minute bars
pub fn detect_impulse_legs(bars_1m: &[Bar], daily_levels: &[DailyLevels]) -> Vec<ImpulseLeg> {
    if bars_1m.len() < SWING_LOOKBACK + MAX_FAST_CANDLES {
        return Vec::new();
    }

    let mut impulse_legs = Vec::new();

    // Find swing highs and lows
    let swing_highs = find_swing_highs(bars_1m, SWING_LOOKBACK);
    let swing_lows = find_swing_lows(bars_1m, SWING_LOOKBACK);

    // Scan for potential impulse moves
    let mut i = SWING_LOOKBACK;
    while i < bars_1m.len() {
        // Try to find impulse starting at this bar
        if let Some(leg) = try_detect_impulse_at(
            bars_1m,
            i,
            &swing_highs,
            &swing_lows,
            daily_levels,
        ) {
            if leg.score_total >= MIN_IMPULSE_SCORE {
                let end_idx = i + leg.num_candles;
                impulse_legs.push(leg);
                i = end_idx; // Skip past this impulse
                continue;
            }
        }
        i += 1;
    }

    impulse_legs
}

fn try_detect_impulse_at(
    bars: &[Bar],
    start_idx: usize,
    swing_highs: &[f64],
    swing_lows: &[f64],
    _daily_levels: &[DailyLevels],
) -> Option<ImpulseLeg> {
    let start_bar = &bars[start_idx];

    // Look for moves of 3-5 candles
    for num_candles in 3..=MAX_FAST_CANDLES.min(bars.len() - start_idx) {
        let end_idx = start_idx + num_candles - 1;
        let end_bar = &bars[end_idx];
        let move_bars = &bars[start_idx..=end_idx];

        // Calculate price move
        let price_change = end_bar.close - start_bar.open;
        let direction = if price_change > 0.0 {
            ImpulseDirection::Up
        } else {
            ImpulseDirection::Down
        };

        let move_size = price_change.abs();

        // Skip if move is too small
        if move_size < MIN_IMPULSE_POINTS {
            continue;
        }

        // Score the move
        let sufficient_size = move_size >= MIN_IMPULSE_POINTS;

        let was_fast = num_candles <= MAX_FAST_CANDLES;

        let broke_swing = check_broke_swing(
            direction,
            start_bar.open,
            end_bar.close,
            swing_highs,
            swing_lows,
            start_idx,
        );

        let uniform_candles = check_uniform_candles(move_bars, direction);

        let volume_increased = check_volume_increase(move_bars, bars, start_idx);

        let score_total = [
            broke_swing,
            was_fast,
            uniform_candles,
            volume_increased,
            sufficient_size,
        ]
        .iter()
        .filter(|&&x| x)
        .count() as u8;

        let total_volume: u64 = move_bars.iter().map(|b| b.volume).sum();

        return Some(ImpulseLeg {
            start_time: start_bar.timestamp,
            end_time: end_bar.timestamp,
            start_price: start_bar.open,
            end_price: end_bar.close,
            direction,
            symbol: start_bar.symbol.clone(),
            date: start_bar.timestamp.date_naive(),
            score_total,
            broke_swing,
            was_fast,
            uniform_candles,
            volume_increased,
            sufficient_size,
            num_candles,
            total_volume,
            avg_volume_per_bar: total_volume / num_candles as u64,
        });
    }

    None
}

fn find_swing_highs(bars: &[Bar], lookback: usize) -> Vec<f64> {
    let mut swing_highs = vec![f64::MIN; bars.len()];

    for i in lookback..bars.len() {
        let high = bars[i - lookback..i]
            .iter()
            .map(|b| b.high)
            .fold(f64::MIN, f64::max);
        swing_highs[i] = high;
    }

    swing_highs
}

fn find_swing_lows(bars: &[Bar], lookback: usize) -> Vec<f64> {
    let mut swing_lows = vec![f64::MAX; bars.len()];

    for i in lookback..bars.len() {
        let low = bars[i - lookback..i]
            .iter()
            .map(|b| b.low)
            .fold(f64::MAX, f64::min);
        swing_lows[i] = low;
    }

    swing_lows
}

fn check_broke_swing(
    direction: ImpulseDirection,
    start_price: f64,
    end_price: f64,
    swing_highs: &[f64],
    swing_lows: &[f64],
    idx: usize,
) -> bool {
    match direction {
        ImpulseDirection::Up => {
            // Check if we broke above the prior swing high
            if idx < swing_highs.len() && swing_highs[idx] != f64::MIN {
                end_price > swing_highs[idx]
            } else {
                false
            }
        }
        ImpulseDirection::Down => {
            // Check if we broke below the prior swing low
            if idx < swing_lows.len() && swing_lows[idx] != f64::MAX {
                end_price < swing_lows[idx]
            } else {
                false
            }
        }
    }
}

fn check_uniform_candles(bars: &[Bar], direction: ImpulseDirection) -> bool {
    if bars.is_empty() {
        return false;
    }

    // Count candles matching the direction
    let matching_candles = bars
        .iter()
        .filter(|b| match direction {
            ImpulseDirection::Up => b.is_bullish(),
            ImpulseDirection::Down => !b.is_bullish(),
        })
        .count();

    // At least 70% of candles should match direction
    let match_ratio = matching_candles as f64 / bars.len() as f64;
    if match_ratio < 0.7 {
        return false;
    }

    // Check for minimal overlap (bodies don't overlap much)
    let mut overlap_count = 0;
    for i in 1..bars.len() {
        let prev = &bars[i - 1];
        let curr = &bars[i];

        let prev_body_low = prev.open.min(prev.close);
        let prev_body_high = prev.open.max(prev.close);
        let curr_body_low = curr.open.min(curr.close);
        let curr_body_high = curr.open.max(curr.close);

        // Check if current body overlaps with previous body
        let overlaps = curr_body_low < prev_body_high && curr_body_high > prev_body_low;
        if overlaps {
            overlap_count += 1;
        }
    }

    // Less than 50% overlap is acceptable
    let overlap_ratio = overlap_count as f64 / (bars.len() - 1).max(1) as f64;
    overlap_ratio < 0.5
}

fn check_volume_increase(move_bars: &[Bar], all_bars: &[Bar], start_idx: usize) -> bool {
    if start_idx < SWING_LOOKBACK {
        return false;
    }

    // Average volume of the impulse move
    let move_avg_volume: f64 = move_bars.iter().map(|b| b.volume as f64).sum::<f64>()
        / move_bars.len() as f64;

    // Average volume of prior bars
    let prior_bars = &all_bars[start_idx - SWING_LOOKBACK..start_idx];
    let prior_avg_volume: f64 = prior_bars.iter().map(|b| b.volume as f64).sum::<f64>()
        / prior_bars.len() as f64;

    // Volume should be at least 20% higher
    move_avg_volume > prior_avg_volume * 1.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_impulse_direction() {
        assert_eq!(ImpulseDirection::Up, ImpulseDirection::Up);
    }
}
