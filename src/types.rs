use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: String,
    pub price: f64,
    pub size: u32,
    pub side: String, // "buy" or "sell"
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bubble {
    pub id: String,
    pub price: f64,
    pub size: u32, // Dominant side volume (aggression)
    pub side: String, // "buy" or "sell"
    pub timestamp: u64,
    pub x: f64,
    pub opacity: f64,
    #[serde(rename = "isSignificantImbalance")]
    pub is_significant_imbalance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CVDPoint {
    pub timestamp: u64,
    pub value: i64,
    pub x: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeProfileLevel {
    pub price: f64,
    #[serde(rename = "buyVolume")]
    pub buy_volume: u32,
    #[serde(rename = "sellVolume")]
    pub sell_volume: u32,
    #[serde(rename = "totalVolume")]
    pub total_volume: u32,
}

/// Absorption Zone - tracks absorption at a specific price level over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsorptionZone {
    pub price: f64,
    #[serde(rename = "absorptionType")]
    pub absorption_type: String,
    #[serde(rename = "totalAbsorbed")]
    pub total_absorbed: i64,
    #[serde(rename = "eventCount")]
    pub event_count: u32,
    #[serde(rename = "firstSeen")]
    pub first_seen: u64,
    #[serde(rename = "lastSeen")]
    pub last_seen: u64,
    pub strength: String, // "weak", "medium", "strong", "defended"
    #[serde(rename = "atPoc")]
    pub at_poc: bool,
    #[serde(rename = "atVah")]
    pub at_vah: bool,
    #[serde(rename = "atVal")]
    pub at_val: bool,
    #[serde(rename = "againstTrend")]
    pub against_trend: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbsorptionEvent {
    pub timestamp: u64,
    pub price: f64,
    #[serde(rename = "absorptionType")]
    pub absorption_type: String,
    pub delta: i64,
    #[serde(rename = "priceChange")]
    pub price_change: f64,
    pub strength: String,
    #[serde(rename = "eventCount")]
    pub event_count: u32,
    #[serde(rename = "totalAbsorbed")]
    pub total_absorbed: i64,
    #[serde(rename = "atKeyLevel")]
    pub at_key_level: bool,
    #[serde(rename = "againstTrend")]
    pub against_trend: bool,
    pub x: f64,
}

/// Delta Flip Event - CVD crossing zero or reversing direction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaFlip {
    pub timestamp: u64,
    #[serde(rename = "flipType")]
    pub flip_type: String, // "zero_cross" or "reversal"
    pub direction: String, // "bullish" (crossing up/reversing up) or "bearish"
    #[serde(rename = "cvdBefore")]
    pub cvd_before: i64,
    #[serde(rename = "cvdAfter")]
    pub cvd_after: i64,
    pub x: f64,
}

/// Stacked Imbalances - 3+ consecutive price levels with same-direction imbalance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackedImbalance {
    pub timestamp: u64,
    pub side: String, // "buy" or "sell"
    #[serde(rename = "levelCount")]
    pub level_count: u32, // How many consecutive levels (3+)
    #[serde(rename = "priceHigh")]
    pub price_high: f64,
    #[serde(rename = "priceLow")]
    pub price_low: f64,
    #[serde(rename = "totalImbalance")]
    pub total_imbalance: i64, // Sum of imbalances across levels
    pub x: f64,
}

/// Confluence Event - Multiple signals aligning for high-probability setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfluenceEvent {
    pub timestamp: u64,
    pub price: f64,
    pub direction: String, // "bullish" or "bearish"
    pub score: u8,         // 2 = medium, 3 = high, 4+ = very high
    pub signals: Vec<String>, // List of contributing signals
    #[serde(rename = "priceAfter1m")]
    pub price_after_1m: Option<f64>, // Filled in later for stats
    #[serde(rename = "priceAfter5m")]
    pub price_after_5m: Option<f64>,
    pub x: f64,
}

/// Signal record for tracking outcomes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalRecord {
    pub timestamp: u64,
    pub price: f64,
    pub signal_type: String, // "delta_flip", "absorption", "stacked_imbalance", "confluence"
    pub direction: String,   // "bullish" or "bearish"
    #[serde(rename = "priceAfter1m")]
    pub price_after_1m: Option<f64>,
    #[serde(rename = "priceAfter5m")]
    pub price_after_5m: Option<f64>,
    pub outcome: Option<String>, // "win", "loss", "breakeven" - filled after 5m
}

/// Session Statistics - aggregated stats for all signals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    #[serde(rename = "sessionStart")]
    pub session_start: u64,
    #[serde(rename = "deltaFlips")]
    pub delta_flips: SignalStats,
    pub absorptions: SignalStats,
    #[serde(rename = "stackedImbalances")]
    pub stacked_imbalances: SignalStats,
    pub confluences: SignalStats,
    #[serde(rename = "currentPrice")]
    pub current_price: f64,
    #[serde(rename = "sessionHigh")]
    pub session_high: f64,
    #[serde(rename = "sessionLow")]
    pub session_low: f64,
    #[serde(rename = "totalVolume")]
    pub total_volume: u64,
}

/// Stats for a specific signal type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignalStats {
    pub count: u32,
    #[serde(rename = "bullishCount")]
    pub bullish_count: u32,
    #[serde(rename = "bearishCount")]
    pub bearish_count: u32,
    pub wins: u32,
    pub losses: u32,
    #[serde(rename = "avgMove1m")]
    pub avg_move_1m: f64, // Average price move after 1 minute
    #[serde(rename = "avgMove5m")]
    pub avg_move_5m: f64, // Average price move after 5 minutes
    #[serde(rename = "winRate")]
    pub win_rate: f64, // Percentage of signals that resulted in expected direction
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    Bubble(Bubble),
    CVDPoint(CVDPoint),
    VolumeProfile { levels: Vec<VolumeProfileLevel> },
    Absorption(AbsorptionEvent),
    AbsorptionZones { zones: Vec<AbsorptionZone> },
    DeltaFlip(DeltaFlip),
    StackedImbalance(StackedImbalance),
    Confluence(ConfluenceEvent),
    SessionStats(SessionStats),
    Connected { symbols: Vec<String> },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub action: String,
    pub symbol: Option<String>,
    pub min_size: Option<u32>,
}

/// Shared application state
pub struct AppState {
    pub tx: broadcast::Sender<WsMessage>,
    pub active_symbols: RwLock<HashSet<String>>,
    pub min_size: RwLock<u32>,
}
