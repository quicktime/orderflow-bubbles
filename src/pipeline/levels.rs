use crate::bars::Bar;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Daily reference levels for a trading session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyLevels {
    pub date: NaiveDate,
    pub symbol: String,

    // Prior day levels (for next day reference)
    pub pdh: f64, // Prior Day High
    pub pdl: f64, // Prior Day Low
    pub pdc: f64, // Prior Day Close

    // Volume Profile levels (computed from current day)
    pub poc: f64, // Point of Control - price with highest volume
    pub vah: f64, // Value Area High - upper bound of 70% volume
    pub val: f64, // Value Area Low - lower bound of 70% volume

    // Session stats
    pub session_high: f64,
    pub session_low: f64,
    pub session_open: f64,
    pub session_close: f64,
    pub total_volume: u64,
}

/// Trading session boundaries (CME NQ futures)
/// Regular Trading Hours: 9:30 AM - 4:00 PM ET (14:30 - 21:00 UTC)
/// Full session: 6:00 PM - 5:00 PM ET next day
const RTH_START_HOUR: u32 = 14; // 9:30 AM ET = 14:30 UTC
const RTH_START_MIN: u32 = 30;
const RTH_END_HOUR: u32 = 21; // 4:00 PM ET = 21:00 UTC

/// Price bucket size for volume profile (NQ tick = 0.25)
const PRICE_BUCKET_SIZE: f64 = 1.0; // 1 point buckets for cleaner profile

pub fn compute_daily_levels(bars: &[Bar]) -> Vec<DailyLevels> {
    if bars.is_empty() {
        return Vec::new();
    }

    // Group bars by trading date (use RTH session date)
    let mut daily_bars: BTreeMap<NaiveDate, Vec<&Bar>> = BTreeMap::new();

    for bar in bars {
        // Use the bar's date as the trading date
        // For proper session handling, we'd need to map overnight sessions
        let date = bar.timestamp.date_naive();
        daily_bars.entry(date).or_default().push(bar);
    }

    let mut levels_list = Vec::new();
    let dates: Vec<_> = daily_bars.keys().cloned().collect();

    for (i, date) in dates.iter().enumerate() {
        let bars = daily_bars.get(date).unwrap();
        if bars.is_empty() {
            continue;
        }

        let symbol = bars[0].symbol.clone();

        // Compute current day's session stats
        let session_high = bars.iter().map(|b| b.high).fold(f64::MIN, f64::max);
        let session_low = bars.iter().map(|b| b.low).fold(f64::MAX, f64::min);
        let session_open = bars.first().map(|b| b.open).unwrap_or(0.0);
        let session_close = bars.last().map(|b| b.close).unwrap_or(0.0);
        let total_volume: u64 = bars.iter().map(|b| b.volume).sum();

        // Get prior day levels (from previous day in our data)
        let (pdh, pdl, pdc) = if i > 0 {
            let prev_date = &dates[i - 1];
            let prev_bars = daily_bars.get(prev_date).unwrap();
            (
                prev_bars.iter().map(|b| b.high).fold(f64::MIN, f64::max),
                prev_bars.iter().map(|b| b.low).fold(f64::MAX, f64::min),
                prev_bars.last().map(|b| b.close).unwrap_or(0.0),
            )
        } else {
            // First day in dataset - use current day's open as reference
            (session_high, session_low, session_open)
        };

        // Compute volume profile
        let (poc, vah, val) = compute_volume_profile(bars);

        levels_list.push(DailyLevels {
            date: *date,
            symbol,
            pdh,
            pdl,
            pdc,
            poc,
            vah,
            val,
            session_high,
            session_low,
            session_open,
            session_close,
            total_volume,
        });
    }

    levels_list
}

/// Build volume profile and compute POC, VAH, VAL
fn compute_volume_profile(bars: &[&Bar]) -> (f64, f64, f64) {
    if bars.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Build volume at price histogram
    let mut volume_at_price: HashMap<i64, u64> = HashMap::new();

    for bar in bars {
        // Distribute bar volume across the bar's range
        // For simplicity, put all volume at VWAP-ish price (midpoint)
        let bar_mid = (bar.high + bar.low) / 2.0;
        let bucket = price_to_bucket(bar_mid);
        *volume_at_price.entry(bucket).or_insert(0) += bar.volume;
    }

    if volume_at_price.is_empty() {
        let price = bars[0].close;
        return (price, price, price);
    }

    // Find POC (bucket with max volume)
    let (poc_bucket, _) = volume_at_price
        .iter()
        .max_by_key(|(_, vol)| *vol)
        .unwrap();
    let poc = bucket_to_price(*poc_bucket);

    // Compute Value Area (70% of total volume)
    let total_volume: u64 = volume_at_price.values().sum();
    let target_volume = (total_volume as f64 * 0.70) as u64;

    // Sort buckets by price
    let mut sorted_buckets: Vec<_> = volume_at_price.iter().collect();
    sorted_buckets.sort_by_key(|(bucket, _)| *bucket);

    // Expand from POC to find value area
    let poc_idx = sorted_buckets
        .iter()
        .position(|(b, _)| *b == poc_bucket)
        .unwrap_or(0);

    let mut val_idx = poc_idx;
    let mut vah_idx = poc_idx;
    let mut accumulated_volume = *volume_at_price.get(poc_bucket).unwrap_or(&0);

    while accumulated_volume < target_volume {
        let can_go_lower = val_idx > 0;
        let can_go_higher = vah_idx < sorted_buckets.len() - 1;

        if !can_go_lower && !can_go_higher {
            break;
        }

        let lower_vol = if can_go_lower {
            *sorted_buckets[val_idx - 1].1
        } else {
            0
        };

        let upper_vol = if can_go_higher {
            *sorted_buckets[vah_idx + 1].1
        } else {
            0
        };

        if lower_vol >= upper_vol && can_go_lower {
            val_idx -= 1;
            accumulated_volume += lower_vol;
        } else if can_go_higher {
            vah_idx += 1;
            accumulated_volume += upper_vol;
        } else if can_go_lower {
            val_idx -= 1;
            accumulated_volume += lower_vol;
        }
    }

    let val = bucket_to_price(*sorted_buckets[val_idx].0);
    let vah = bucket_to_price(*sorted_buckets[vah_idx].0);

    (poc, vah, val)
}

fn price_to_bucket(price: f64) -> i64 {
    (price / PRICE_BUCKET_SIZE).round() as i64
}

fn bucket_to_price(bucket: i64) -> f64 {
    bucket as f64 * PRICE_BUCKET_SIZE
}

/// Check if a price is within a tolerance of a level
pub fn is_near_level(price: f64, level: f64, tolerance: f64) -> bool {
    (price - level).abs() <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_volume_profile() {
        let ts = Utc::now();
        let bars: Vec<&Bar> = vec![];
        // Would need actual bar data for meaningful test
    }
}
