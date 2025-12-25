//! Local Replay Mode
//!
//! Streams trades from local .zst files through ProcessingState.
//! No Databento API connection required - uses downloaded historical data.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

use crate::processing::ProcessingState;
use crate::types::{AppState, Trade, WsMessage};

/// Trade record from Databento CSV
#[derive(Debug, Deserialize)]
struct CsvTrade {
    ts_recv: String,
    ts_event: String,
    rtype: u8,
    publisher_id: u32,
    instrument_id: u64,
    action: String,
    side: String,
    depth: u8,
    price: f64,
    size: u64,
    flags: u32,
    ts_in_delta: i64,
    sequence: u64,
    symbol: String,
}

/// Find all .zst files in directory for a specific date
pub fn find_trade_files(data_dir: &PathBuf, date_filter: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(data_dir)
        .with_context(|| format!("Failed to read directory: {:?}", data_dir))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "zst") {
            if let Some(filter) = date_filter {
                let filename = path.file_name().unwrap().to_string_lossy();
                if !filename.contains(filter) {
                    continue;
                }
            }
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

/// Parse trades from a zstd-compressed CSV file
fn parse_zst_trades(path: &PathBuf) -> Result<Vec<Trade>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file: {:?}", path))?;

    let decoder = zstd::stream::Decoder::new(file)
        .with_context(|| format!("Failed to create zstd decoder for: {:?}", path))?;

    let reader = BufReader::new(decoder);
    let mut csv_reader = csv::Reader::from_reader(reader);

    let mut trades = Vec::new();

    for result in csv_reader.deserialize() {
        let row: CsvTrade = result.with_context(|| "Failed to parse CSV row")?;

        // Only process trade actions
        if row.action != "T" {
            continue;
        }

        let side = match row.side.as_str() {
            "B" => "buy",
            "A" => "sell",
            _ => continue,
        };

        // Parse timestamp
        let ts_event = DateTime::parse_from_rfc3339(&row.ts_event)
            .with_context(|| format!("Failed to parse timestamp: {}", row.ts_event))?
            .with_timezone(&Utc);

        trades.push(Trade {
            symbol: row.symbol,
            price: row.price,
            size: row.size as u32,
            side: side.to_string(),
            timestamp: ts_event.timestamp_millis() as u64,
        });
    }

    // Sort by timestamp
    trades.sort_by_key(|t| t.timestamp);

    Ok(trades)
}

/// Local replay mode: Stream trades from local .zst files through ProcessingState
pub async fn run_local_replay(
    data_dir: PathBuf,
    replay_date: Option<String>,
    replay_speed: u32,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Starting local replay from {:?}", data_dir);

    // Find trade files
    let date_filter = replay_date.as_ref().map(|d| d.replace("-", ""));
    let files = find_trade_files(&data_dir, date_filter.as_deref())?;

    if files.is_empty() {
        anyhow::bail!("No .zst trade files found in {:?}", data_dir);
    }

    info!("Found {} trade files", files.len());

    // Load all trades
    let mut all_trades = Vec::new();
    for file in &files {
        info!("Loading trades from {:?}", file);
        let trades = parse_zst_trades(file)?;
        info!("  Loaded {} trades", trades.len());
        all_trades.extend(trades);
    }

    // Sort all trades by timestamp
    all_trades.sort_by_key(|t| t.timestamp);
    info!("Total trades to replay: {}", all_trades.len());

    if all_trades.is_empty() {
        anyhow::bail!("No trades found in files");
    }

    // Get symbols from trades
    let symbols: Vec<String> = all_trades
        .iter()
        .map(|t| t.symbol.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Notify clients we're connected (in replay mode)
    let _ = state.tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
        mode: state.mode.clone(),
    });

    // Create processing state with Supabase persistence and AppState for stats sync
    let processing_state = Arc::new(RwLock::new(ProcessingState::new(
        state.supabase.clone(),
        state.session_id,
        Some(state.clone()),
    )));

    // Spawn aggregation task (with speed multiplier)
    let processing_state_clone = processing_state.clone();
    let tx_clone = state.tx.clone();
    let speed = replay_speed;
    tokio::spawn(async move {
        let interval_ms = 1000 / speed as u64;
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms.max(50)));
        loop {
            interval.tick().await;
            let mut pstate = processing_state_clone.write().await;
            pstate.process_buffer(&tx_clone);
            pstate.send_volume_profile(&tx_clone);
        }
    });

    // Track timestamps for pacing
    let mut last_trade_ts: Option<u64> = None;
    let total_trades = all_trades.len();

    // Process each trade
    for (idx, trade) in all_trades.into_iter().enumerate() {
        // Check pause state
        loop {
            let ctrl = state.replay_control.read().await;
            if !ctrl.is_paused {
                break;
            }
            drop(ctrl);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let trade_ts = trade.timestamp;

        // Update current timestamp in replay control
        {
            let mut ctrl = state.replay_control.write().await;
            ctrl.current_timestamp = Some(trade_ts);
        }

        // Get current speed from shared state
        let current_speed = state.replay_control.read().await.speed;

        // Pace the trades according to their original timing (adjusted by speed)
        if let Some(last_ts) = last_trade_ts {
            if trade_ts > last_ts {
                let delay_ms = (trade_ts - last_ts) / current_speed as u64;
                if delay_ms > 0 && delay_ms < 5000 {
                    // Cap at 5 seconds to skip gaps
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
        last_trade_ts = Some(trade_ts);

        // Check min size filter
        let min_size = *state.min_size.read().await;
        if trade.size < min_size {
            continue;
        }

        // Add trade to processing state
        {
            let mut pstate = processing_state.write().await;
            pstate.add_trade(trade);
        }

        // Log progress periodically
        if idx % 10000 == 0 {
            info!("Replay progress: {}/{} trades ({:.1}%)",
                  idx, total_trades, (idx as f64 / total_trades as f64) * 100.0);
        }
    }

    info!("Local replay complete!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_trade_files() {
        // Would need test data
    }
}
