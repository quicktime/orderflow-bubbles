use std::collections::HashMap;
use tokio::sync::broadcast;
use tracing::info;

use crate::types::{
    AbsorptionEvent, AbsorptionZone, Bubble, CVDPoint, ConfluenceEvent, DeltaFlip,
    SessionStats, SignalRecord, SignalStats, StackedImbalance, Trade, VolumeProfileLevel,
    WsMessage,
};

/// Volume snapshot for rolling average calculation
#[derive(Debug, Clone)]
struct VolumeSnapshot {
    timestamp: u64,
    volume: u32,
    #[allow(dead_code)]
    delta: i64,
}

/// Internal absorption zone tracking (more fields than we send to client)
#[derive(Debug, Clone)]
struct AbsorptionZoneInternal {
    price: f64,
    #[allow(dead_code)]
    price_key: i64,
    absorption_type: String,
    total_absorbed: i64,
    event_count: u32,
    first_seen: u64,
    last_seen: u64,
    peak_strength: u8, // 0=weak, 1=medium, 2=strong, 3=defended - never goes down
}

/// Processing state for trade aggregation
pub struct ProcessingState {
    trade_buffer: Vec<Trade>,
    bubble_counter: u64,
    cvd: i64,
    volume_profile: HashMap<i64, VolumeProfileLevel>, // Key = price * 4 (for 0.25 tick size)
    total_buy_volume: u64,
    total_sell_volume: u64,

    // Enhanced absorption detection
    window_first_price: Option<f64>,
    window_last_price: Option<f64>,

    // Rolling volume for dynamic thresholds (last 60 seconds)
    volume_history: Vec<VolumeSnapshot>,

    // Absorption zones by price level (key = price * 4)
    absorption_zones: HashMap<i64, AbsorptionZoneInternal>,

    // CVD trend tracking (for context)
    cvd_5s_ago: i64, // CVD from 5 seconds ago for trend detection
    cvd_history: Vec<(u64, i64)>, // (timestamp, cvd) for trend calculation

    // Delta flip detection
    prev_cvd_sign: i8, // -1 = negative, 0 = zero, 1 = positive
    last_delta_flip_time: u64, // Prevent rapid-fire flip events (cooldown)

    // Stacked imbalances tracking
    last_stacked_imbalance_time: u64, // Cooldown to prevent spam
    last_stacked_imbalance_side: Option<String>, // Track last emitted to avoid duplicates

    // === CONFLUENCE & STATISTICS ===
    // Signal history for confluence detection and outcome tracking
    signal_history: Vec<SignalRecord>,
    // Recent signals within confluence window (5 seconds)
    recent_signals: Vec<(u64, String, String, f64)>, // (timestamp, signal_type, direction, price)
    // Session tracking
    session_start: u64,
    session_high: f64,
    session_low: f64,
    current_price: f64,
    // Last stats broadcast time (throttle to every 5 seconds)
    last_stats_broadcast: u64,
    // Last confluence time (cooldown)
    last_confluence_time: u64,
}

impl ProcessingState {
    pub fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            trade_buffer: Vec::new(),
            bubble_counter: 0,
            cvd: 0,
            volume_profile: HashMap::new(),
            total_buy_volume: 0,
            total_sell_volume: 0,
            window_first_price: None,
            window_last_price: None,
            volume_history: Vec::new(),
            absorption_zones: HashMap::new(),
            cvd_5s_ago: 0,
            cvd_history: Vec::new(),
            prev_cvd_sign: 0,
            last_delta_flip_time: 0,
            last_stacked_imbalance_time: 0,
            last_stacked_imbalance_side: None,
            // Confluence & stats
            signal_history: Vec::new(),
            recent_signals: Vec::new(),
            session_start: now,
            session_high: 0.0,
            session_low: f64::MAX,
            current_price: 0.0,
            last_stats_broadcast: 0,
            last_confluence_time: 0,
        }
    }

    /// Calculate rolling average volume per second over last N seconds
    fn get_avg_volume_per_second(&self, seconds: u64) -> f64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cutoff = now.saturating_sub(seconds * 1000);

        let recent: Vec<_> = self
            .volume_history
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .collect();

        if recent.is_empty() {
            return 200.0; // Default baseline for NQ
        }

        let total_vol: u32 = recent.iter().map(|s| s.volume).sum();
        total_vol as f64 / seconds as f64
    }

    /// Get CVD trend direction: positive = bullish, negative = bearish
    fn get_cvd_trend(&self) -> i64 {
        self.cvd - self.cvd_5s_ago
    }

    /// Find POC (Point of Control) - price with highest volume
    fn get_poc(&self) -> Option<f64> {
        self.volume_profile
            .values()
            .max_by_key(|l| l.total_volume)
            .map(|l| l.price)
    }

    /// Get VAH/VAL (Value Area High/Low) - 70% of volume
    fn get_value_area(&self) -> Option<(f64, f64)> {
        if self.volume_profile.is_empty() {
            return None;
        }

        let total_vol: u32 = self.volume_profile.values().map(|l| l.total_volume).sum();
        let target_vol = (total_vol as f64 * 0.7) as u32;

        let poc = self.get_poc()?;
        let poc_key = (poc * 4.0).round() as i64;

        let mut included_vol = self.volume_profile.get(&poc_key)?.total_volume;
        let mut high_key = poc_key;
        let mut low_key = poc_key;

        // Expand outward from POC until we have 70% of volume
        while included_vol < target_vol {
            let above_key = high_key + 1;
            let below_key = low_key - 1;

            let above_vol = self
                .volume_profile
                .get(&above_key)
                .map(|l| l.total_volume)
                .unwrap_or(0);
            let below_vol = self
                .volume_profile
                .get(&below_key)
                .map(|l| l.total_volume)
                .unwrap_or(0);

            if above_vol == 0 && below_vol == 0 {
                break;
            }

            if above_vol >= below_vol {
                high_key = above_key;
                included_vol += above_vol;
            } else {
                low_key = below_key;
                included_vol += below_vol;
            }
        }

        Some((high_key as f64 / 4.0, low_key as f64 / 4.0))
    }

    /// Check if price is at a key level (POC, VAH, VAL)
    fn is_at_key_level(&self, price: f64) -> (bool, bool, bool) {
        let poc = self.get_poc();
        let va = self.get_value_area();

        let tolerance = 0.5; // Within 2 ticks

        let at_poc = poc.map(|p| (price - p).abs() <= tolerance).unwrap_or(false);
        let at_vah = va
            .map(|(h, _)| (price - h).abs() <= tolerance)
            .unwrap_or(false);
        let at_val = va
            .map(|(_, l)| (price - l).abs() <= tolerance)
            .unwrap_or(false);

        (at_poc, at_vah, at_val)
    }

    /// Calculate strength based on event count and context - returns (string, numeric)
    fn calculate_strength_with_num(
        &self,
        event_count: u32,
        at_key_level: bool,
        against_trend: bool,
    ) -> (&'static str, u8) {
        let base_strength = match event_count {
            1 => 0,
            2 => 1,
            3 => 2,
            _ => 3,
        };

        let bonus = (if at_key_level { 1 } else { 0 }) + (if against_trend { 1 } else { 0 });
        let total = base_strength + bonus;

        match total {
            0 => ("weak", 0),
            1 => ("medium", 1),
            2 => ("strong", 2),
            _ => ("defended", 3),
        }
    }

    /// Convert numeric strength to string
    fn strength_num_to_str(num: u8) -> &'static str {
        match num {
            0 => "weak",
            1 => "medium",
            2 => "strong",
            _ => "defended",
        }
    }

    /// Clean up old absorption zones with strength-based persistence (using peak_strength)
    /// - Weak/Medium (0-1): 5 minutes
    /// - Strong (2): 15 minutes
    /// - Defended (3): 30 minutes
    fn cleanup_old_zones(&mut self, now: u64) {
        self.absorption_zones.retain(|_, zone| {
            let max_age_ms = match zone.peak_strength {
                0..=1 => 5 * 60 * 1000,  // 5 minutes for weak/medium
                2 => 15 * 60 * 1000,     // 15 minutes for strong
                _ => 30 * 60 * 1000,     // 30 minutes for defended
            };
            let cutoff = now.saturating_sub(max_age_ms);
            zone.last_seen >= cutoff
        });
    }

    /// Clean up old volume history (older than 60 seconds)
    fn cleanup_volume_history(&mut self, now: u64) {
        let cutoff = now.saturating_sub(60 * 1000);
        self.volume_history.retain(|s| s.timestamp >= cutoff);
    }

    /// Clean up old CVD history (older than 30 seconds)
    fn cleanup_cvd_history(&mut self, now: u64) {
        let cutoff = now.saturating_sub(30 * 1000);
        self.cvd_history.retain(|(ts, _)| *ts >= cutoff);

        // Update cvd_5s_ago
        let target = now.saturating_sub(5000);
        self.cvd_5s_ago = self
            .cvd_history
            .iter()
            .filter(|(ts, _)| *ts <= target)
            .max_by_key(|(ts, _)| *ts)
            .map(|(_, cvd)| *cvd)
            .unwrap_or(self.cvd);
    }

    /// Add a trade to the processing buffer
    pub fn add_trade(&mut self, trade: Trade) {
        // Update CVD
        let delta = if trade.side == "buy" {
            trade.size as i64
        } else {
            -(trade.size as i64)
        };
        self.cvd += delta;

        // Update volume totals
        if trade.side == "buy" {
            self.total_buy_volume += trade.size as u64;
        } else {
            self.total_sell_volume += trade.size as u64;
        }

        // Track first and last price for absorption detection
        if self.window_first_price.is_none() {
            self.window_first_price = Some(trade.price);
        }
        self.window_last_price = Some(trade.price);

        // Update volume profile (0.25 tick size)
        let price_key = (trade.price * 4.0).round() as i64;
        let rounded_price = price_key as f64 / 4.0;

        self.volume_profile
            .entry(price_key)
            .and_modify(|level| {
                if trade.side == "buy" {
                    level.buy_volume += trade.size;
                } else {
                    level.sell_volume += trade.size;
                }
                level.total_volume += trade.size;
            })
            .or_insert(VolumeProfileLevel {
                price: rounded_price,
                buy_volume: if trade.side == "buy" { trade.size } else { 0 },
                sell_volume: if trade.side == "sell" { trade.size } else { 0 },
                total_volume: trade.size,
            });

        // Add to buffer for aggregation
        self.trade_buffer.push(trade);
    }

    /// Process the trade buffer and emit bubbles, CVD points, and absorption events
    pub fn process_buffer(&mut self, tx: &broadcast::Sender<WsMessage>) {
        if self.trade_buffer.is_empty() {
            return;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Cleanup old data
        self.cleanup_old_zones(now);
        self.cleanup_volume_history(now);
        self.cleanup_cvd_history(now);

        // Aggregate by side
        let mut total_buy_volume = 0u32;
        let mut total_sell_volume = 0u32;
        let mut buy_trades = Vec::new();
        let mut sell_trades = Vec::new();

        for trade in &self.trade_buffer {
            if trade.side == "buy" {
                total_buy_volume += trade.size;
                buy_trades.push(trade);
            } else {
                total_sell_volume += trade.size;
                sell_trades.push(trade);
            }
        }

        let total_volume = total_buy_volume + total_sell_volume;
        if total_volume == 0 {
            self.trade_buffer.clear();
            return;
        }

        // Calculate delta and determine dominant side
        let delta = total_buy_volume as i64 - total_sell_volume as i64;
        let dominant_side = if delta > 0 { "buy" } else { "sell" };
        let dominant_volume = if delta > 0 {
            total_buy_volume
        } else {
            total_sell_volume
        };

        // Calculate volume-weighted average price for dominant side
        let dominant_trades = if delta > 0 { &buy_trades } else { &sell_trades };
        let avg_price = if !dominant_trades.is_empty() {
            let weighted_sum: f64 = dominant_trades
                .iter()
                .map(|t| t.price * t.size as f64)
                .sum();
            let total_size: u32 = dominant_trades.iter().map(|t| t.size).sum();
            weighted_sum / total_size as f64
        } else {
            self.trade_buffer.iter().map(|t| t.price).sum::<f64>() / self.trade_buffer.len() as f64
        };

        // Store volume snapshot for rolling average
        self.volume_history.push(VolumeSnapshot {
            timestamp: now,
            volume: total_volume,
            delta,
        });

        // Store CVD for trend tracking
        self.cvd_history.push((now, self.cvd));

        // Determine if imbalance is significant (> 15% of total volume)
        let imbalance_ratio = delta.abs() as f64 / total_volume as f64;
        let is_significant_imbalance = imbalance_ratio > 0.15;

        // Create bubble
        let bubble = Bubble {
            id: format!("bubble-{}", self.bubble_counter),
            price: avg_price,
            size: dominant_volume,
            side: dominant_side.to_string(),
            timestamp: now,
            x: 0.92,
            opacity: 1.0,
            is_significant_imbalance,
        };

        self.bubble_counter += 1;

        // Send bubble
        let _ = tx.send(WsMessage::Bubble(bubble));

        // Send CVD point
        let cvd_point = CVDPoint {
            timestamp: now,
            value: self.cvd,
            x: 0.92,
        };
        let _ = tx.send(WsMessage::CVDPoint(cvd_point));

        // === DELTA FLIP DETECTION ===
        let current_cvd_sign = if self.cvd > 0 {
            1i8
        } else if self.cvd < 0 {
            -1i8
        } else {
            0i8
        };

        // Detect zero-cross (sign change through zero)
        // Cooldown of 2 seconds to prevent rapid-fire events
        let cooldown_ms = 2000;
        if self.prev_cvd_sign != 0
            && current_cvd_sign != 0
            && self.prev_cvd_sign != current_cvd_sign
            && now.saturating_sub(self.last_delta_flip_time) > cooldown_ms
        {
            let direction = if current_cvd_sign > 0 {
                "bullish"
            } else {
                "bearish"
            };

            let delta_flip = DeltaFlip {
                timestamp: now,
                flip_type: "zero_cross".to_string(),
                direction: direction.to_string(),
                cvd_before: self.cvd_5s_ago,
                cvd_after: self.cvd,
                x: 0.92,
            };

            let _ = tx.send(WsMessage::DeltaFlip(delta_flip));
            self.last_delta_flip_time = now;

            info!(
                "⚡ DELTA FLIP [{}]: CVD crossed zero → {} (was {}, now {})",
                direction.to_uppercase(),
                direction,
                self.cvd_5s_ago,
                self.cvd
            );

            // Record for confluence detection and stats
            self.record_signal(tx, now, "delta_flip", direction, avg_price);
        }

        self.prev_cvd_sign = current_cvd_sign;

        // === STACKED IMBALANCES DETECTION ===
        // Look for 3+ consecutive price levels with same-direction imbalance
        self.detect_stacked_imbalances(tx, now);

        // === ENHANCED ABSORPTION DETECTION ===
        if let (Some(first_price), Some(last_price)) =
            (self.window_first_price, self.window_last_price)
        {
            let price_change = last_price - first_price;
            let abs_delta = delta.abs();

            // Dynamic threshold based on rolling average volume
            // Absorption requires delta > 40% of average volume per second
            let avg_vol = self.get_avg_volume_per_second(30);
            let min_delta_threshold = (avg_vol * 0.4).max(20.0) as i64;

            // Price movement threshold - 1 tick (0.25 for NQ)
            const PRICE_THRESHOLD: f64 = 0.25;

            if abs_delta >= min_delta_threshold {
                // Check for absorption:
                // - Buying absorbed: delta > 0 but price didn't go up (or went down)
                // - Selling absorbed: delta < 0 but price didn't go down (or went up)
                let is_buying_absorbed = delta > 0 && price_change <= PRICE_THRESHOLD;
                let is_selling_absorbed = delta < 0 && price_change >= -PRICE_THRESHOLD;

                if is_buying_absorbed || is_selling_absorbed {
                    let absorption_type = if is_buying_absorbed {
                        "buying"
                    } else {
                        "selling"
                    };
                    let price_key = (avg_price * 4.0).round() as i64;

                    // Get context
                    let (at_poc, at_vah, at_val) = self.is_at_key_level(avg_price);
                    let at_key_level = at_poc || at_vah || at_val;
                    let cvd_trend = self.get_cvd_trend();

                    // Against trend: buying absorbed during bullish trend, or selling absorbed during bearish trend
                    let against_trend = (is_buying_absorbed && cvd_trend > 100)
                        || (is_selling_absorbed && cvd_trend < -100);

                    // Update or create absorption zone
                    let zone = self
                        .absorption_zones
                        .entry(price_key)
                        .or_insert_with(|| AbsorptionZoneInternal {
                            price: avg_price,
                            price_key,
                            absorption_type: absorption_type.to_string(),
                            total_absorbed: 0,
                            event_count: 0,
                            first_seen: now,
                            last_seen: now,
                            peak_strength: 0,
                        });

                    zone.total_absorbed += abs_delta;
                    zone.event_count += 1;
                    zone.last_seen = now;
                    zone.absorption_type = absorption_type.to_string();

                    // Copy values before releasing borrow
                    let zone_event_count = zone.event_count;
                    let zone_total_absorbed = zone.total_absorbed;
                    let zone_peak_strength = zone.peak_strength;

                    // Calculate current strength (now we don't hold mutable borrow)
                    let (strength, strength_num) =
                        self.calculate_strength_with_num(zone_event_count, at_key_level, against_trend);

                    // Update peak strength if current is higher (never goes down)
                    if strength_num > zone_peak_strength {
                        if let Some(z) = self.absorption_zones.get_mut(&price_key) {
                            z.peak_strength = strength_num;
                        }
                    }

                    // Only emit events for medium+ strength OR first event at key level
                    let should_emit = strength != "weak" || (zone_event_count == 1 && at_key_level);

                    if should_emit {
                        let absorption_event = AbsorptionEvent {
                            timestamp: now,
                            price: avg_price,
                            absorption_type: absorption_type.to_string(),
                            delta,
                            price_change,
                            strength: strength.to_string(),
                            event_count: zone_event_count,
                            total_absorbed: zone_total_absorbed,
                            at_key_level,
                            against_trend,
                            x: 0.92,
                        };

                        let _ = tx.send(WsMessage::Absorption(absorption_event));

                        let context_str = match (at_poc, at_vah, at_val, against_trend) {
                            (true, _, _, true) => "@ POC ⚠️ AGAINST TREND",
                            (true, _, _, false) => "@ POC",
                            (_, true, _, true) => "@ VAH ⚠️ AGAINST TREND",
                            (_, true, _, false) => "@ VAH",
                            (_, _, true, true) => "@ VAL ⚠️ AGAINST TREND",
                            (_, _, true, false) => "@ VAL",
                            (_, _, _, true) => "⚠️ AGAINST TREND",
                            _ => "",
                        };

                        info!(
                            "🛡️ ABSORPTION [{}]: {} absorbed at {:.2} | events={} total={}  {} {}",
                            strength.to_uppercase(),
                            absorption_type,
                            avg_price,
                            zone_event_count,
                            zone_total_absorbed,
                            if zone_event_count >= 3 {
                                "🔥 DEFENDED LEVEL"
                            } else {
                                ""
                            },
                            context_str
                        );

                        // Record for confluence detection and stats
                        // Buying absorbed = bearish (sellers absorbing), Selling absorbed = bullish (buyers absorbing)
                        let abs_direction = if is_buying_absorbed {
                            "bearish"
                        } else {
                            "bullish"
                        };
                        self.record_signal(tx, now, "absorption", abs_direction, avg_price);
                    }
                }
            }
        }

        // Send absorption zones update (only active ones)
        let zones: Vec<AbsorptionZone> = self
            .absorption_zones
            .values()
            .filter(|z| z.event_count >= 2) // Only send zones with 2+ events
            .map(|z| {
                let (at_poc, at_vah, at_val) = self.is_at_key_level(z.price);
                let cvd_trend = self.get_cvd_trend();
                let against_trend = (z.absorption_type == "buying" && cvd_trend > 100)
                    || (z.absorption_type == "selling" && cvd_trend < -100);

                // Use peak_strength - once defended, always defended
                let strength = Self::strength_num_to_str(z.peak_strength);

                AbsorptionZone {
                    price: z.price,
                    absorption_type: z.absorption_type.clone(),
                    total_absorbed: z.total_absorbed,
                    event_count: z.event_count,
                    first_seen: z.first_seen,
                    last_seen: z.last_seen,
                    strength: strength.to_string(),
                    at_poc,
                    at_vah,
                    at_val,
                    against_trend,
                }
            })
            .collect();

        if !zones.is_empty() {
            let _ = tx.send(WsMessage::AbsorptionZones { zones });
        }

        // Reset window price tracking
        self.window_first_price = None;
        self.window_last_price = None;

        // Clear buffer
        self.trade_buffer.clear();

        info!(
            "Created bubble: {} aggression={} ({:.0}% imbalance) {}",
            dominant_side,
            dominant_volume,
            imbalance_ratio * 100.0,
            if is_significant_imbalance {
                "COLORED"
            } else {
                "grey"
            }
        );
    }

    /// Detect stacked imbalances from session volume profile
    /// Uses 1-point buckets, looks for 3+ consecutive levels with 70%+ dominance
    fn detect_stacked_imbalances(&mut self, tx: &broadcast::Sender<WsMessage>, now: u64) {
        // 30 second cooldown between emissions
        const COOLDOWN_MS: u64 = 30_000;
        if now.saturating_sub(self.last_stacked_imbalance_time) < COOLDOWN_MS {
            return;
        }

        if self.volume_profile.is_empty() {
            return;
        }

        // Aggregate into 1-point buckets (4 ticks = 1 point for NQ)
        let mut point_buckets: HashMap<i64, (u32, u32)> = HashMap::new();
        for level in self.volume_profile.values() {
            let point_key = level.price.floor() as i64; // 1-point buckets
            point_buckets
                .entry(point_key)
                .and_modify(|(buy, sell)| {
                    *buy += level.buy_volume;
                    *sell += level.sell_volume;
                })
                .or_insert((level.buy_volume, level.sell_volume));
        }

        // Sort by price
        let mut levels: Vec<_> = point_buckets.into_iter().collect();
        levels.sort_by_key(|(key, _)| *key);

        // Minimum 70% dominance to count as imbalanced
        const MIN_IMBALANCE_RATIO: f64 = 0.70;
        // Minimum volume at a level to consider it (filter noise)
        const MIN_LEVEL_VOLUME: u32 = 100;

        let mut best_streak_side: Option<&str> = None;
        let mut best_streak: Vec<(i64, i64)> = Vec::new();
        let mut current_streak_side: Option<&str> = None;
        let mut current_streak: Vec<(i64, i64)> = Vec::new();

        for (price_key, (buy_vol, sell_vol)) in &levels {
            let total = buy_vol + sell_vol;
            if total < MIN_LEVEL_VOLUME {
                // Check if current streak is better than best
                if current_streak.len() > best_streak.len() && current_streak.len() >= 3 {
                    best_streak = current_streak.clone();
                    best_streak_side = current_streak_side;
                }
                current_streak_side = None;
                current_streak.clear();
                continue;
            }

            let buy_ratio = *buy_vol as f64 / total as f64;
            let level_side = if buy_ratio >= MIN_IMBALANCE_RATIO {
                Some("buy")
            } else if buy_ratio <= (1.0 - MIN_IMBALANCE_RATIO) {
                Some("sell")
            } else {
                None
            };

            let level_delta = *buy_vol as i64 - *sell_vol as i64;

            match (level_side, current_streak_side) {
                (Some(side), Some(streak_side)) if side == streak_side => {
                    current_streak.push((*price_key, level_delta));
                }
                (Some(side), _) => {
                    // Different side or starting fresh - save best if applicable
                    if current_streak.len() > best_streak.len() && current_streak.len() >= 3 {
                        best_streak = current_streak.clone();
                        best_streak_side = current_streak_side;
                    }
                    current_streak_side = Some(side);
                    current_streak.clear();
                    current_streak.push((*price_key, level_delta));
                }
                (None, _) => {
                    if current_streak.len() > best_streak.len() && current_streak.len() >= 3 {
                        best_streak = current_streak.clone();
                        best_streak_side = current_streak_side;
                    }
                    current_streak_side = None;
                    current_streak.clear();
                }
            }
        }

        // Check final streak
        if current_streak.len() > best_streak.len() && current_streak.len() >= 3 {
            best_streak = current_streak;
            best_streak_side = current_streak_side;
        }

        // Only emit if we found a significant stack (4+ levels) and it's different from last
        if best_streak.len() >= 4 {
            if let Some(side) = best_streak_side {
                // Check if this is different from what we last emitted
                let dominated_by = side.to_string();
                if self.last_stacked_imbalance_side.as_ref() != Some(&dominated_by) {
                    let price_low = best_streak.first().unwrap().0 as f64;
                    let price_high = best_streak.last().unwrap().0 as f64 + 1.0; // +1 for bucket end
                    let total_imbalance: i64 = best_streak.iter().map(|(_, delta)| delta.abs()).sum();

                    let stacked = StackedImbalance {
                        timestamp: now,
                        side: side.to_string(),
                        level_count: best_streak.len() as u32,
                        price_high,
                        price_low,
                        total_imbalance,
                        x: 0.92,
                    };

                    let _ = tx.send(WsMessage::StackedImbalance(stacked));

                    self.last_stacked_imbalance_time = now;
                    self.last_stacked_imbalance_side = Some(side.to_string());

                    info!(
                        "📊 STACKED IMBALANCE [{}]: {} consecutive 1-point levels from {:.0} to {:.0} | total imbalance={}",
                        side.to_uppercase(),
                        best_streak.len(),
                        price_low,
                        price_high,
                        total_imbalance
                    );

                    // Record for confluence detection and stats
                    let stacked_direction = if side == "buy" { "bullish" } else { "bearish" };
                    let mid_price = (price_low + price_high) / 2.0;
                    self.record_signal(tx, now, "stacked_imbalance", stacked_direction, mid_price);
                }
            }
        }
    }

    /// Record a signal for confluence detection and stats tracking
    fn record_signal(
        &mut self,
        tx: &broadcast::Sender<WsMessage>,
        now: u64,
        signal_type: &str,
        direction: &str,
        price: f64,
    ) {
        // Update session high/low
        if price > self.session_high {
            self.session_high = price;
        }
        if price < self.session_low && price > 0.0 {
            self.session_low = price;
        }
        self.current_price = price;

        // Add to recent signals for confluence detection
        self.recent_signals
            .push((now, signal_type.to_string(), direction.to_string(), price));

        // Clean old signals (older than 5 seconds)
        let cutoff = now.saturating_sub(5000);
        self.recent_signals.retain(|(ts, _, _, _)| *ts >= cutoff);

        // Add to signal history for stats
        let record = SignalRecord {
            timestamp: now,
            price,
            signal_type: signal_type.to_string(),
            direction: direction.to_string(),
            price_after_1m: None,
            price_after_5m: None,
            outcome: None,
        };
        self.signal_history.push(record);

        // Detect confluence (multiple signals within 5 seconds)
        self.detect_confluence(tx, now, price);

        // Update outcomes for past signals
        self.update_signal_outcomes(now, price);

        // Broadcast stats every 5 seconds
        if now.saturating_sub(self.last_stats_broadcast) >= 5000 {
            self.broadcast_stats(tx, now);
            self.last_stats_broadcast = now;
        }
    }

    /// Detect confluence - multiple signals aligning within time window
    fn detect_confluence(&mut self, tx: &broadcast::Sender<WsMessage>, now: u64, price: f64) {
        // Cooldown of 10 seconds between confluence events
        if now.saturating_sub(self.last_confluence_time) < 10_000 {
            return;
        }

        // Need at least 2 different signal types within 5 seconds
        if self.recent_signals.len() < 2 {
            return;
        }

        // Group signals by type
        let mut signal_types: HashMap<String, Vec<(u64, String)>> = HashMap::new();
        for (ts, sig_type, direction, _) in &self.recent_signals {
            signal_types
                .entry(sig_type.clone())
                .or_default()
                .push((*ts, direction.clone()));
        }

        // Need at least 2 different signal types
        if signal_types.len() < 2 {
            return;
        }

        // Determine consensus direction
        let mut bullish_count = 0;
        let mut bearish_count = 0;
        let mut signals: Vec<String> = Vec::new();

        for (sig_type, occurrences) in &signal_types {
            // Take the most recent occurrence of each signal type
            if let Some((_, direction)) = occurrences.last() {
                signals.push(sig_type.clone());
                if direction == "bullish" {
                    bullish_count += 1;
                } else {
                    bearish_count += 1;
                }
            }
        }

        // Need consensus direction (at least 2 agreeing)
        let direction = if bullish_count >= 2 {
            "bullish"
        } else if bearish_count >= 2 {
            "bearish"
        } else {
            return; // No consensus
        };

        let score = signals.len() as u8;

        // Create confluence event
        let confluence = ConfluenceEvent {
            timestamp: now,
            price,
            direction: direction.to_string(),
            score,
            signals: signals.clone(),
            price_after_1m: None,
            price_after_5m: None,
            x: 0.92,
        };

        let _ = tx.send(WsMessage::Confluence(confluence));
        self.last_confluence_time = now;

        // Also record confluence as a signal for stats
        let record = SignalRecord {
            timestamp: now,
            price,
            signal_type: "confluence".to_string(),
            direction: direction.to_string(),
            price_after_1m: None,
            price_after_5m: None,
            outcome: None,
        };
        self.signal_history.push(record);

        info!(
            "🎯 CONFLUENCE [{}]: {} signals agree → {} | score={} | signals: {:?}",
            if score >= 3 { "HIGH" } else { "MEDIUM" },
            score,
            direction.to_uppercase(),
            score,
            signals
        );

        // Clear recent signals to avoid re-triggering
        self.recent_signals.clear();
    }

    /// Update past signals with price outcomes (1m and 5m after)
    fn update_signal_outcomes(&mut self, now: u64, current_price: f64) {
        for record in &mut self.signal_history {
            // Update 1-minute price if 1 minute has passed
            if record.price_after_1m.is_none() && now.saturating_sub(record.timestamp) >= 60_000 {
                record.price_after_1m = Some(current_price);
            }

            // Update 5-minute price and determine outcome
            if record.price_after_5m.is_none() && now.saturating_sub(record.timestamp) >= 300_000 {
                record.price_after_5m = Some(current_price);

                // Determine outcome based on direction
                let move_amount = current_price - record.price;
                let min_move = 2.0; // Minimum 2 points for meaningful move

                record.outcome = Some(
                    if record.direction == "bullish" {
                        if move_amount >= min_move {
                            "win"
                        } else if move_amount <= -min_move {
                            "loss"
                        } else {
                            "breakeven"
                        }
                    } else {
                        // bearish
                        if move_amount <= -min_move {
                            "win"
                        } else if move_amount >= min_move {
                            "loss"
                        } else {
                            "breakeven"
                        }
                    }
                    .to_string(),
                );
            }
        }

        // Cleanup old signals (older than 30 minutes)
        let cutoff = now.saturating_sub(30 * 60 * 1000);
        self.signal_history.retain(|r| r.timestamp >= cutoff);
    }

    /// Calculate stats for a specific signal type
    fn calculate_signal_stats(&self, signal_type: &str) -> SignalStats {
        let signals: Vec<_> = self
            .signal_history
            .iter()
            .filter(|r| r.signal_type == signal_type)
            .collect();

        if signals.is_empty() {
            return SignalStats::default();
        }

        let count = signals.len() as u32;
        let bullish_count = signals.iter().filter(|r| r.direction == "bullish").count() as u32;
        let bearish_count = count - bullish_count;

        let wins = signals
            .iter()
            .filter(|r| r.outcome.as_deref() == Some("win"))
            .count() as u32;
        let losses = signals
            .iter()
            .filter(|r| r.outcome.as_deref() == Some("loss"))
            .count() as u32;

        // Calculate average moves
        let moves_1m: Vec<f64> = signals
            .iter()
            .filter_map(|r| r.price_after_1m.map(|p| p - r.price))
            .collect();
        let moves_5m: Vec<f64> = signals
            .iter()
            .filter_map(|r| r.price_after_5m.map(|p| p - r.price))
            .collect();

        let avg_move_1m = if moves_1m.is_empty() {
            0.0
        } else {
            moves_1m.iter().sum::<f64>() / moves_1m.len() as f64
        };

        let avg_move_5m = if moves_5m.is_empty() {
            0.0
        } else {
            moves_5m.iter().sum::<f64>() / moves_5m.len() as f64
        };

        let completed = wins + losses;
        let win_rate = if completed > 0 {
            (wins as f64 / completed as f64) * 100.0
        } else {
            0.0
        };

        SignalStats {
            count,
            bullish_count,
            bearish_count,
            wins,
            losses,
            avg_move_1m,
            avg_move_5m,
            win_rate,
        }
    }

    /// Broadcast session stats to clients
    fn broadcast_stats(&self, tx: &broadcast::Sender<WsMessage>, _now: u64) {
        let stats = SessionStats {
            session_start: self.session_start,
            delta_flips: self.calculate_signal_stats("delta_flip"),
            absorptions: self.calculate_signal_stats("absorption"),
            stacked_imbalances: self.calculate_signal_stats("stacked_imbalance"),
            confluences: self.calculate_signal_stats("confluence"),
            current_price: self.current_price,
            session_high: if self.session_high > 0.0 {
                self.session_high
            } else {
                self.current_price
            },
            session_low: if self.session_low < f64::MAX {
                self.session_low
            } else {
                self.current_price
            },
            total_volume: self.total_buy_volume + self.total_sell_volume,
        };

        let _ = tx.send(WsMessage::SessionStats(stats));
    }

    /// Send the current volume profile to clients
    pub fn send_volume_profile(&self, tx: &broadcast::Sender<WsMessage>) {
        let levels: Vec<VolumeProfileLevel> = self.volume_profile.values().cloned().collect();
        let _ = tx.send(WsMessage::VolumeProfile { levels });
    }
}

impl Default for ProcessingState {
    fn default() -> Self {
        Self::new()
    }
}
