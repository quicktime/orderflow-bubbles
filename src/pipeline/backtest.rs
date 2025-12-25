//! Backtesting Module
//!
//! Analyzes trading strategy performance with configurable parameters.
//! Computes statistics: Win Rate, Profit, R:R, Profit Factor, etc.

use crate::bars::Bar;
use crate::levels::DailyLevels;
use crate::replay::CapturedSignal;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strategy configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Minimum confluence score to enter trade
    pub min_confluence_score: u8,

    /// Required signal types for entry (e.g., ["delta_flip", "absorption"])
    pub required_signals: Vec<String>,

    /// Stop loss in points
    pub stop_loss_points: f64,

    /// Take profit in points
    pub take_profit_points: f64,

    /// Maximum hold time in seconds before forced exit
    pub max_hold_time_secs: u64,

    /// Only trade at key levels (POC, VAH, VAL)
    pub require_key_level: bool,

    /// Only take signals with strength >= this level
    pub min_strength: Option<String>,

    /// Time-of-day filter (e.g., only RTH)
    pub rth_only: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            min_confluence_score: 2,
            required_signals: vec![],
            stop_loss_points: 10.0,
            take_profit_points: 20.0,
            max_hold_time_secs: 300, // 5 minutes
            require_key_level: false,
            min_strength: None,
            rth_only: true,
        }
    }
}

/// Single trade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub entry_time: u64,
    pub exit_time: u64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub direction: String,  // "long" or "short"
    pub signal_type: String,
    pub pnl_points: f64,
    pub pnl_ticks: i32,     // NQ: 1 point = 4 ticks, 1 tick = $5
    pub exit_reason: String, // "stop_loss", "take_profit", "timeout", "signal_exit"
    pub max_favorable_excursion: f64,  // MFE - how much it went in your favor
    pub max_adverse_excursion: f64,    // MAE - how much it went against you
}

impl TradeResult {
    pub fn is_winner(&self) -> bool {
        self.pnl_points > 0.0
    }

    pub fn risk_reward(&self) -> f64 {
        if self.max_adverse_excursion == 0.0 {
            return 0.0;
        }
        self.max_favorable_excursion / self.max_adverse_excursion
    }

    /// Dollar P&L for NQ (1 tick = $5, 4 ticks = 1 point = $20)
    pub fn pnl_dollars(&self) -> f64 {
        self.pnl_points * 20.0
    }
}

/// Backtest results summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub config: StrategyConfig,
    pub trades: Vec<TradeResult>,

    // Summary statistics
    pub total_trades: u32,
    pub winners: u32,
    pub losers: u32,
    pub breakeven: u32,
    pub win_rate: f64,

    // P&L metrics
    pub total_pnl_points: f64,
    pub total_pnl_dollars: f64,
    pub avg_win_points: f64,
    pub avg_loss_points: f64,
    pub profit_factor: f64,   // Gross profit / Gross loss
    pub avg_rr: f64,          // Average risk:reward

    // Risk metrics
    pub max_drawdown_points: f64,
    pub max_drawdown_dollars: f64,
    pub sharpe_ratio: f64,    // Simplified daily Sharpe
    pub max_consecutive_losses: u32,
    pub max_consecutive_wins: u32,

    // Time analysis
    pub avg_hold_time_secs: f64,
    pub best_hour: Option<u32>,
    pub worst_hour: Option<u32>,
}

/// Backtester engine
pub struct Backtester {
    config: StrategyConfig,
    bars: Vec<Bar>,
    daily_levels: HashMap<NaiveDate, DailyLevels>,
    price_index: HashMap<u64, usize>, // timestamp -> bar index for fast lookup
}

impl Backtester {
    pub fn new(config: StrategyConfig, bars: Vec<Bar>, levels: Vec<DailyLevels>) -> Self {
        let daily_levels: HashMap<_, _> = levels.into_iter()
            .map(|l| (l.date, l))
            .collect();

        // Build price index for O(1) lookups
        let price_index: HashMap<_, _> = bars.iter()
            .enumerate()
            .map(|(i, b)| (b.timestamp.timestamp_millis() as u64, i))
            .collect();

        Self {
            config,
            bars,
            daily_levels,
            price_index,
        }
    }

    /// Get price at a specific timestamp (or nearest bar after)
    fn get_price_at(&self, timestamp_ms: u64) -> Option<f64> {
        // Find the bar at or after this timestamp
        if let Some(&idx) = self.price_index.get(&(timestamp_ms / 1000 * 1000)) {
            return Some(self.bars[idx].close);
        }

        // Linear search for nearest bar (could optimize with sorted vec)
        self.bars.iter()
            .find(|b| b.timestamp.timestamp_millis() as u64 >= timestamp_ms)
            .map(|b| b.close)
    }

    /// Get bars in a time range
    fn get_bars_in_range(&self, start_ms: u64, end_ms: u64) -> Vec<&Bar> {
        self.bars.iter()
            .filter(|b| {
                let ts = b.timestamp.timestamp_millis() as u64;
                ts >= start_ms && ts <= end_ms
            })
            .collect()
    }

    /// Check if timestamp is during RTH (9:30 AM - 4:00 PM ET)
    fn is_rth(&self, timestamp_ms: u64) -> bool {
        let ts = DateTime::from_timestamp_millis(timestamp_ms as i64)
            .unwrap_or_else(|| Utc::now());
        let hour = ts.time().hour();
        // Approximate RTH in UTC: 14:30 - 21:00
        hour >= 14 && hour < 21
    }

    /// Check if price is near a key level
    fn is_at_key_level(&self, price: f64, date: NaiveDate) -> bool {
        if let Some(levels) = self.daily_levels.get(&date) {
            let tolerance = 2.0; // Within 2 points
            return (price - levels.poc).abs() <= tolerance
                || (price - levels.vah).abs() <= tolerance
                || (price - levels.val).abs() <= tolerance
                || (price - levels.pdh).abs() <= tolerance
                || (price - levels.pdl).abs() <= tolerance;
        }
        false
    }

    /// Check if signal passes the strategy filter
    fn signal_passes_filter(&self, signal: &CapturedSignal) -> bool {
        // RTH filter
        if self.config.rth_only && !self.is_rth(signal.timestamp) {
            return false;
        }

        // Required signals filter
        if !self.config.required_signals.is_empty() {
            if !self.config.required_signals.contains(&signal.signal_type) {
                return false;
            }
        }

        // Minimum strength filter
        if let Some(ref min_strength) = self.config.min_strength {
            if let Some(ref strength) = signal.strength {
                let strength_order = ["weak", "medium", "strong", "defended"];
                let min_idx = strength_order.iter().position(|s| s == min_strength).unwrap_or(0);
                let sig_idx = strength_order.iter().position(|s| s == strength).unwrap_or(0);
                if sig_idx < min_idx {
                    return false;
                }
            }
        }

        // Confluence score filter
        if signal.signal_type == "confluence" {
            if let Some(ref strength) = signal.strength {
                if strength.starts_with("score_") {
                    if let Ok(score) = strength.trim_start_matches("score_").parse::<u8>() {
                        if score < self.config.min_confluence_score {
                            return false;
                        }
                    }
                }
            }
        }

        // Key level filter
        if self.config.require_key_level && signal.price > 0.0 {
            let date = DateTime::from_timestamp_millis(signal.timestamp as i64)
                .map(|dt| dt.date_naive())
                .unwrap_or_else(|| NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
            if !self.is_at_key_level(signal.price, date) {
                return false;
            }
        }

        true
    }

    /// Simulate a trade from signal
    fn simulate_trade(&self, signal: &CapturedSignal) -> Option<TradeResult> {
        let entry_price = if signal.price > 0.0 {
            signal.price
        } else {
            self.get_price_at(signal.timestamp)?
        };

        let direction = signal.direction.clone();
        let is_long = direction == "bullish";

        let stop_price = if is_long {
            entry_price - self.config.stop_loss_points
        } else {
            entry_price + self.config.stop_loss_points
        };

        let target_price = if is_long {
            entry_price + self.config.take_profit_points
        } else {
            entry_price - self.config.take_profit_points
        };

        let max_hold_ms = self.config.max_hold_time_secs * 1000;
        let exit_deadline = signal.timestamp + max_hold_ms;

        // Get bars from entry to max hold time
        let trade_bars = self.get_bars_in_range(signal.timestamp, exit_deadline);

        if trade_bars.is_empty() {
            return None;
        }

        let mut exit_time = exit_deadline;
        let mut exit_price = trade_bars.last().map(|b| b.close)?;
        let mut exit_reason = "timeout".to_string();
        let mut max_favorable = 0.0f64;
        let mut max_adverse = 0.0f64;

        for bar in &trade_bars {
            let bar_ts = bar.timestamp.timestamp_millis() as u64;

            // Track MFE/MAE
            if is_long {
                max_favorable = max_favorable.max(bar.high - entry_price);
                max_adverse = max_adverse.max(entry_price - bar.low);

                // Check stop loss
                if bar.low <= stop_price {
                    exit_time = bar_ts;
                    exit_price = stop_price;
                    exit_reason = "stop_loss".to_string();
                    break;
                }

                // Check take profit
                if bar.high >= target_price {
                    exit_time = bar_ts;
                    exit_price = target_price;
                    exit_reason = "take_profit".to_string();
                    break;
                }
            } else {
                max_favorable = max_favorable.max(entry_price - bar.low);
                max_adverse = max_adverse.max(bar.high - entry_price);

                // Check stop loss
                if bar.high >= stop_price {
                    exit_time = bar_ts;
                    exit_price = stop_price;
                    exit_reason = "stop_loss".to_string();
                    break;
                }

                // Check take profit
                if bar.low <= target_price {
                    exit_time = bar_ts;
                    exit_price = target_price;
                    exit_reason = "take_profit".to_string();
                    break;
                }
            }
        }

        let pnl_points = if is_long {
            exit_price - entry_price
        } else {
            entry_price - exit_price
        };

        Some(TradeResult {
            entry_time: signal.timestamp,
            exit_time,
            entry_price,
            exit_price,
            direction: if is_long { "long" } else { "short" }.to_string(),
            signal_type: signal.signal_type.clone(),
            pnl_points,
            pnl_ticks: (pnl_points * 4.0).round() as i32,
            exit_reason,
            max_favorable_excursion: max_favorable,
            max_adverse_excursion: max_adverse,
        })
    }

    /// Run backtest on signals
    pub fn run(&self, signals: &[CapturedSignal]) -> BacktestResults {
        let mut trades = Vec::new();
        let mut last_exit_time = 0u64;

        for signal in signals {
            // Skip if we're still in a trade
            if signal.timestamp < last_exit_time {
                continue;
            }

            // Apply filters
            if !self.signal_passes_filter(signal) {
                continue;
            }

            // Simulate trade
            if let Some(trade) = self.simulate_trade(signal) {
                last_exit_time = trade.exit_time;
                trades.push(trade);
            }
        }

        // Calculate statistics
        self.calculate_statistics(trades)
    }

    fn calculate_statistics(&self, trades: Vec<TradeResult>) -> BacktestResults {
        let total_trades = trades.len() as u32;

        if total_trades == 0 {
            return BacktestResults {
                config: self.config.clone(),
                trades: vec![],
                total_trades: 0,
                winners: 0,
                losers: 0,
                breakeven: 0,
                win_rate: 0.0,
                total_pnl_points: 0.0,
                total_pnl_dollars: 0.0,
                avg_win_points: 0.0,
                avg_loss_points: 0.0,
                profit_factor: 0.0,
                avg_rr: 0.0,
                max_drawdown_points: 0.0,
                max_drawdown_dollars: 0.0,
                sharpe_ratio: 0.0,
                max_consecutive_losses: 0,
                max_consecutive_wins: 0,
                avg_hold_time_secs: 0.0,
                best_hour: None,
                worst_hour: None,
            };
        }

        let winners: Vec<_> = trades.iter().filter(|t| t.pnl_points > 0.5).collect();
        let losers: Vec<_> = trades.iter().filter(|t| t.pnl_points < -0.5).collect();
        let breakeven: Vec<_> = trades.iter().filter(|t| t.pnl_points.abs() <= 0.5).collect();

        let win_count = winners.len() as u32;
        let loss_count = losers.len() as u32;
        let be_count = breakeven.len() as u32;

        let win_rate = if total_trades > 0 {
            (win_count as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let gross_profit: f64 = winners.iter().map(|t| t.pnl_points).sum();
        let gross_loss: f64 = losers.iter().map(|t| t.pnl_points.abs()).sum();

        let avg_win = if !winners.is_empty() {
            gross_profit / winners.len() as f64
        } else {
            0.0
        };

        let avg_loss = if !losers.is_empty() {
            gross_loss / losers.len() as f64
        } else {
            0.0
        };

        let profit_factor = if gross_loss > 0.0 {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let total_pnl_points: f64 = trades.iter().map(|t| t.pnl_points).sum();
        let total_pnl_dollars = total_pnl_points * 20.0;

        // Calculate drawdown
        let mut peak = 0.0f64;
        let mut max_dd = 0.0f64;
        let mut cumulative = 0.0f64;
        for trade in &trades {
            cumulative += trade.pnl_points;
            peak = peak.max(cumulative);
            max_dd = max_dd.max(peak - cumulative);
        }

        // Calculate consecutive wins/losses
        let mut max_consec_wins = 0u32;
        let mut max_consec_losses = 0u32;
        let mut current_consec_wins = 0u32;
        let mut current_consec_losses = 0u32;

        for trade in &trades {
            if trade.is_winner() {
                current_consec_wins += 1;
                current_consec_losses = 0;
                max_consec_wins = max_consec_wins.max(current_consec_wins);
            } else if trade.pnl_points < -0.5 {
                current_consec_losses += 1;
                current_consec_wins = 0;
                max_consec_losses = max_consec_losses.max(current_consec_losses);
            }
        }

        // Average hold time
        let avg_hold_time: f64 = trades.iter()
            .map(|t| (t.exit_time - t.entry_time) as f64 / 1000.0)
            .sum::<f64>() / trades.len() as f64;

        // Average R:R
        let avg_rr: f64 = trades.iter()
            .map(|t| t.risk_reward())
            .sum::<f64>() / trades.len() as f64;

        // Hour analysis
        let mut pnl_by_hour: HashMap<u32, f64> = HashMap::new();
        for trade in &trades {
            if let Some(dt) = DateTime::from_timestamp_millis(trade.entry_time as i64) {
                let hour = dt.time().hour();
                *pnl_by_hour.entry(hour).or_insert(0.0) += trade.pnl_points;
            }
        }

        let best_hour = pnl_by_hour.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(h, _)| *h);

        let worst_hour = pnl_by_hour.iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(h, _)| *h);

        // Simple Sharpe approximation (daily returns)
        let returns: Vec<f64> = trades.iter().map(|t| t.pnl_points).collect();
        let mean_return = total_pnl_points / trades.len() as f64;
        let variance: f64 = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / trades.len() as f64;
        let std_dev = variance.sqrt();
        let sharpe = if std_dev > 0.0 { mean_return / std_dev } else { 0.0 };

        BacktestResults {
            config: self.config.clone(),
            trades,
            total_trades,
            winners: win_count,
            losers: loss_count,
            breakeven: be_count,
            win_rate,
            total_pnl_points,
            total_pnl_dollars,
            avg_win_points: avg_win,
            avg_loss_points: avg_loss,
            profit_factor,
            avg_rr,
            max_drawdown_points: max_dd,
            max_drawdown_dollars: max_dd * 20.0,
            sharpe_ratio: sharpe,
            max_consecutive_losses: max_consec_losses,
            max_consecutive_wins: max_consec_wins,
            avg_hold_time_secs: avg_hold_time,
            best_hour,
            worst_hour,
        }
    }
}

/// Print backtest results in a readable format
pub fn print_results(results: &BacktestResults) {
    println!("\n═══════════════════════════════════════════════════════════");
    println!("                    BACKTEST RESULTS                        ");
    println!("═══════════════════════════════════════════════════════════\n");

    println!("Strategy Configuration:");
    println!("  Stop Loss:     {:.1} pts", results.config.stop_loss_points);
    println!("  Take Profit:   {:.1} pts", results.config.take_profit_points);
    println!("  Max Hold Time: {} secs", results.config.max_hold_time_secs);
    println!("  RTH Only:      {}", results.config.rth_only);
    println!();

    println!("Trade Statistics:");
    println!("  Total Trades:  {}", results.total_trades);
    println!("  Winners:       {} ({:.1}%)", results.winners, results.win_rate);
    println!("  Losers:        {}", results.losers);
    println!("  Breakeven:     {}", results.breakeven);
    println!();

    println!("P&L Metrics:");
    println!("  Total P&L:     {:.1} pts (${:.2})", results.total_pnl_points, results.total_pnl_dollars);
    println!("  Avg Win:       {:.1} pts", results.avg_win_points);
    println!("  Avg Loss:      {:.1} pts", results.avg_loss_points);
    println!("  Profit Factor: {:.2}", results.profit_factor);
    println!("  Avg R:R:       {:.2}", results.avg_rr);
    println!();

    println!("Risk Metrics:");
    println!("  Max Drawdown:  {:.1} pts (${:.2})", results.max_drawdown_points, results.max_drawdown_dollars);
    println!("  Sharpe Ratio:  {:.2}", results.sharpe_ratio);
    println!("  Max Consec L:  {}", results.max_consecutive_losses);
    println!("  Max Consec W:  {}", results.max_consecutive_wins);
    println!();

    println!("Time Analysis:");
    println!("  Avg Hold Time: {:.1} secs", results.avg_hold_time_secs);
    if let Some(hour) = results.best_hour {
        println!("  Best Hour:     {:02}:00 UTC", hour);
    }
    if let Some(hour) = results.worst_hour {
        println!("  Worst Hour:    {:02}:00 UTC", hour);
    }

    println!("\n═══════════════════════════════════════════════════════════\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StrategyConfig::default();
        assert_eq!(config.stop_loss_points, 10.0);
        assert_eq!(config.take_profit_points, 20.0);
    }

    #[test]
    fn test_trade_result_is_winner() {
        let trade = TradeResult {
            entry_time: 0,
            exit_time: 1000,
            entry_price: 21500.0,
            exit_price: 21510.0,
            direction: "long".to_string(),
            signal_type: "confluence".to_string(),
            pnl_points: 10.0,
            pnl_ticks: 40,
            exit_reason: "take_profit".to_string(),
            max_favorable_excursion: 12.0,
            max_adverse_excursion: 3.0,
        };

        assert!(trade.is_winner());
        assert_eq!(trade.pnl_dollars(), 200.0);
    }
}
