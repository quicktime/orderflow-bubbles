use crate::impulse::ImpulseLeg;
use crate::trades::{Side, Trade};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Price bucket size for volume profile (finer granularity for LVN detection)
const LVN_BUCKET_SIZE: f64 = 0.5; // 2 ticks = 0.5 points for NQ

/// Threshold for LVN: volume < 30% of average volume at price
const LVN_THRESHOLD_RATIO: f64 = 0.30;

/// Low Volume Node extracted from impulse leg volume profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LvnLevel {
    pub price: f64,
    pub volume: u64,
    pub avg_volume: f64,
    pub volume_ratio: f64, // Actual/Average (< 0.3 qualifies)
    pub impulse_start_time: DateTime<Utc>,
    pub impulse_end_time: DateTime<Utc>,
    pub date: NaiveDate,
    pub symbol: String,
}

/// Extract LVNs from impulse legs by building volume profiles for each leg
pub fn extract_lvns(trades: &[Trade], impulse_legs: &[ImpulseLeg]) -> Vec<LvnLevel> {
    let mut lvn_levels = Vec::new();

    for leg in impulse_legs {
        // Filter trades within this impulse leg's time window
        let leg_trades: Vec<_> = trades
            .iter()
            .filter(|t| t.ts_event >= leg.start_time && t.ts_event <= leg.end_time)
            .collect();

        if leg_trades.is_empty() {
            continue;
        }

        // Build volume profile for this leg
        let mut volume_at_price: HashMap<i64, u64> = HashMap::new();

        for trade in &leg_trades {
            let bucket = price_to_bucket(trade.price);
            *volume_at_price.entry(bucket).or_insert(0) += trade.size;
        }

        if volume_at_price.is_empty() {
            continue;
        }

        // Calculate average volume across all price levels
        let total_volume: u64 = volume_at_price.values().sum();
        let avg_volume = total_volume as f64 / volume_at_price.len() as f64;

        // Find LVNs: price levels with volume < 30% of average
        for (bucket, volume) in &volume_at_price {
            let volume_ratio = *volume as f64 / avg_volume;

            if volume_ratio < LVN_THRESHOLD_RATIO {
                lvn_levels.push(LvnLevel {
                    price: bucket_to_price(*bucket),
                    volume: *volume,
                    avg_volume,
                    volume_ratio,
                    impulse_start_time: leg.start_time,
                    impulse_end_time: leg.end_time,
                    date: leg.date,
                    symbol: leg.symbol.clone(),
                });
            }
        }
    }

    // Sort by price
    lvn_levels.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

    lvn_levels
}

fn price_to_bucket(price: f64) -> i64 {
    (price / LVN_BUCKET_SIZE).round() as i64
}

fn bucket_to_price(bucket: i64) -> f64 {
    bucket as f64 * LVN_BUCKET_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lvn_bucket_conversion() {
        let price = 21500.5;
        let bucket = price_to_bucket(price);
        let recovered = bucket_to_price(bucket);
        assert!((price - recovered).abs() < 0.01);
    }
}
