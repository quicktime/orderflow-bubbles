//! Replay Module
//!
//! Feeds historical trades through the exact same ProcessingState as live trading.
//! This ensures replay behavior matches production 1:1.

use crate::trades::{Side, Trade as PipelineTrade};
use anyhow::Result;
use arrow::array::{ArrayRef, Float64Array, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

// Import from the library crate
use orderflow_bubbles::{ProcessingState, types::WsMessage};

/// Captured signal from replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedSignal {
    pub timestamp: u64,
    pub signal_type: String,
    pub direction: String,
    pub price: f64,
    pub strength: Option<String>,
    pub extra_data: Option<String>,
}

/// Convert pipeline trades to the format expected by ProcessingState
pub fn convert_to_processing_trade(trade: &PipelineTrade) -> orderflow_bubbles::types::Trade {
    orderflow_bubbles::types::Trade {
        symbol: trade.symbol.clone(),
        price: trade.price,
        size: trade.size as u32,
        side: match trade.side {
            Side::Buy => "buy".to_string(),
            Side::Sell => "sell".to_string(),
        },
        timestamp: trade.ts_event.timestamp_millis() as u64,
    }
}

/// Signal collector that captures WsMessage signals for backtesting
pub struct SignalCollector {
    pub signals: Vec<CapturedSignal>,
}

impl SignalCollector {
    pub fn new() -> Self {
        Self {
            signals: Vec::new(),
        }
    }

    /// Process a WsMessage and extract signal if applicable
    pub fn process_message(&mut self, msg: &WsMessage) {
        match msg {
            WsMessage::DeltaFlip(flip) => {
                self.signals.push(CapturedSignal {
                    timestamp: flip.timestamp,
                    signal_type: "delta_flip".to_string(),
                    direction: flip.direction.clone(),
                    price: 0.0, // Delta flips don't have a specific price
                    strength: None,
                    extra_data: Some(format!("cvd: {} -> {}", flip.cvd_before, flip.cvd_after)),
                });
            }
            WsMessage::Absorption(abs) => {
                self.signals.push(CapturedSignal {
                    timestamp: abs.timestamp,
                    signal_type: "absorption".to_string(),
                    direction: if abs.absorption_type == "buying" { "bearish" } else { "bullish" }.to_string(),
                    price: abs.price,
                    strength: Some(abs.strength.clone()),
                    extra_data: Some(format!("delta: {}, events: {}", abs.delta, abs.event_count)),
                });
            }
            WsMessage::StackedImbalance(stacked) => {
                self.signals.push(CapturedSignal {
                    timestamp: stacked.timestamp,
                    signal_type: "stacked_imbalance".to_string(),
                    direction: if stacked.side == "buy" { "bullish" } else { "bearish" }.to_string(),
                    price: (stacked.price_high + stacked.price_low) / 2.0,
                    strength: None,
                    extra_data: Some(format!("levels: {}, range: {:.0}-{:.0}", stacked.level_count, stacked.price_low, stacked.price_high)),
                });
            }
            WsMessage::Confluence(conf) => {
                self.signals.push(CapturedSignal {
                    timestamp: conf.timestamp,
                    signal_type: "confluence".to_string(),
                    direction: conf.direction.clone(),
                    price: conf.price,
                    strength: Some(format!("score_{}", conf.score)),
                    extra_data: Some(conf.signals.join(", ")),
                });
            }
            _ => {} // Ignore non-signal messages (bubbles, CVD, etc.)
        }
    }
}

/// Replay trades through the production ProcessingState
/// Returns captured signals that can be used for backtesting
pub fn replay_trades_for_signals(trades: &[PipelineTrade]) -> Vec<CapturedSignal> {

    if trades.is_empty() {
        return Vec::new();
    }

    info!("Starting replay of {} trades through ProcessingState", trades.len());

    // Create broadcast channel for capturing signals
    let (tx, mut rx) = broadcast::channel::<WsMessage>(10000);

    // Create production ProcessingState (no Supabase, no session)
    let mut state = ProcessingState::new(None, None, None);

    // Signal collector
    let mut collector = SignalCollector::new();

    // Group trades by 100ms windows (simulating real-time aggregation)
    let mut current_window_end = 0u64;
    let window_size_ms = 100; // 100ms windows like production

    for trade in trades {
        let processing_trade = convert_to_processing_trade(trade);
        let trade_ts = processing_trade.timestamp;

        // Check if we need to process the buffer (new window)
        if current_window_end == 0 {
            current_window_end = trade_ts + window_size_ms;
        }

        if trade_ts >= current_window_end {
            // Process the accumulated buffer
            state.process_buffer(&tx);

            // Drain any signals from the channel
            while let Ok(msg) = rx.try_recv() {
                collector.process_message(&msg);
            }

            // Start new window
            current_window_end = trade_ts + window_size_ms;
        }

        // Add trade to buffer
        state.add_trade(processing_trade);
    }

    // Process remaining trades in buffer
    state.process_buffer(&tx);

    // Drain remaining signals
    while let Ok(msg) = rx.try_recv() {
        collector.process_message(&msg);
    }

    info!("Replay complete. Captured {} signals", collector.signals.len());

    // Log signal breakdown
    let delta_flips = collector.signals.iter().filter(|s| s.signal_type == "delta_flip").count();
    let absorptions = collector.signals.iter().filter(|s| s.signal_type == "absorption").count();
    let stacked = collector.signals.iter().filter(|s| s.signal_type == "stacked_imbalance").count();
    let confluences = collector.signals.iter().filter(|s| s.signal_type == "confluence").count();

    info!("Signal breakdown: {} delta_flips, {} absorptions, {} stacked_imbalances, {} confluences",
          delta_flips, absorptions, stacked, confluences);

    collector.signals
}

/// Write captured signals to Parquet file
pub fn write_signals_parquet(signals: &[CapturedSignal], path: &Path) -> Result<()> {
    if signals.is_empty() {
        info!("No signals to write");
        return Ok(());
    }

    let schema = Schema::new(vec![
        Field::new("timestamp", DataType::UInt64, false),
        Field::new("signal_type", DataType::Utf8, false),
        Field::new("direction", DataType::Utf8, false),
        Field::new("price", DataType::Float64, false),
        Field::new("strength", DataType::Utf8, true),
        Field::new("extra_data", DataType::Utf8, true),
    ]);

    let timestamps: Vec<u64> = signals.iter().map(|s| s.timestamp).collect();
    let signal_types: Vec<&str> = signals.iter().map(|s| s.signal_type.as_str()).collect();
    let directions: Vec<&str> = signals.iter().map(|s| s.direction.as_str()).collect();
    let prices: Vec<f64> = signals.iter().map(|s| s.price).collect();
    let strengths: Vec<Option<&str>> = signals.iter().map(|s| s.strength.as_deref()).collect();
    let extra_data: Vec<Option<&str>> = signals.iter().map(|s| s.extra_data.as_deref()).collect();

    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(UInt64Array::from(timestamps)) as ArrayRef,
            Arc::new(StringArray::from(signal_types)) as ArrayRef,
            Arc::new(StringArray::from(directions)) as ArrayRef,
            Arc::new(Float64Array::from(prices)) as ArrayRef,
            Arc::new(StringArray::from(strengths)) as ArrayRef,
            Arc::new(StringArray::from(extra_data)) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_collector() {
        let mut collector = SignalCollector::new();
        assert!(collector.signals.is_empty());
    }
}
