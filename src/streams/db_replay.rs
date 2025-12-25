//! Database Replay Mode
//!
//! Streams trades/bars from Supabase through ProcessingState.
//! Reads processed data that was uploaded by the pipeline.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

use crate::processing::ProcessingState;
use crate::types::{AppState, Trade, WsMessage};

/// Bar record from Supabase (replay_bars_1s table)
#[derive(Debug, Deserialize)]
struct BarRecord {
    timestamp: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: i64,
    buy_volume: i64,
    sell_volume: i64,
    delta: i64,
    trade_count: i64,
    symbol: String,
}

/// Supabase client for fetching replay data
struct ReplayClient {
    client: Client,
    url: String,
    key: String,
}

impl ReplayClient {
    fn from_env() -> Result<Self> {
        let url = std::env::var("SUPABASE_URL")
            .context("SUPABASE_URL not set")?;
        let key = std::env::var("SUPABASE_ANON_KEY")
            .context("SUPABASE_ANON_KEY not set")?;

        Ok(Self {
            client: Client::new(),
            url,
            key,
        })
    }

    /// Fetch bars for a specific date range, ordered by timestamp
    async fn fetch_bars(&self, date_filter: Option<&str>, limit: usize, offset: usize) -> Result<Vec<BarRecord>> {
        let mut url = format!(
            "{}/rest/v1/replay_bars_1s?select=*&order=timestamp.asc&limit={}&offset={}",
            self.url, limit, offset
        );

        // Add date filter if provided (filter by timestamp prefix)
        if let Some(date) = date_filter {
            // Convert YYYY-MM-DD to timestamp prefix filter
            url.push_str(&format!("&timestamp=gte.{}T00:00:00", date));
            url.push_str(&format!("&timestamp=lt.{}T23:59:59", date));
        }

        let response = self.client
            .get(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .send()
            .await
            .context("Failed to fetch bars from Supabase")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Supabase fetch failed ({}): {}", status, text);
        }

        let bars: Vec<BarRecord> = response.json().await
            .context("Failed to parse bars response")?;

        Ok(bars)
    }

    /// Get total count of bars for a date
    async fn count_bars(&self, date_filter: Option<&str>) -> Result<usize> {
        let mut url = format!(
            "{}/rest/v1/replay_bars_1s?select=count",
            self.url
        );

        if let Some(date) = date_filter {
            url.push_str(&format!("&timestamp=gte.{}T00:00:00", date));
            url.push_str(&format!("&timestamp=lt.{}T23:59:59", date));
        }

        let response = self.client
            .get(&url)
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", self.key))
            .header("Prefer", "count=exact")
            .send()
            .await
            .context("Failed to count bars")?;

        // Get count from Content-Range header
        if let Some(range) = response.headers().get("content-range") {
            let range_str = range.to_str().unwrap_or("");
            // Format: "0-999/12345" - we want the total after /
            if let Some(total) = range_str.split('/').last() {
                if let Ok(count) = total.parse::<usize>() {
                    return Ok(count);
                }
            }
        }

        Ok(0)
    }
}

/// Convert bar record to synthetic trades for ProcessingState
fn bar_to_trades(bar: &BarRecord) -> Vec<Trade> {
    // Parse timestamp
    let ts = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(0);

    let mut trades = Vec::new();

    // Create synthetic buy trades
    if bar.buy_volume > 0 {
        trades.push(Trade {
            symbol: bar.symbol.clone(),
            price: bar.close, // Use close price
            size: bar.buy_volume as u32,
            side: "buy".to_string(),
            timestamp: ts,
        });
    }

    // Create synthetic sell trades
    if bar.sell_volume > 0 {
        trades.push(Trade {
            symbol: bar.symbol.clone(),
            price: bar.close,
            size: bar.sell_volume as u32,
            side: "sell".to_string(),
            timestamp: ts,
        });
    }

    trades
}

/// Database replay mode: Stream bars from Supabase through ProcessingState
pub async fn run_db_replay(
    replay_date: Option<String>,
    replay_speed: u32,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Starting database replay from Supabase");

    let client = ReplayClient::from_env()?;

    // Get total count
    let total_bars = client.count_bars(replay_date.as_deref()).await?;
    info!("Found {} bars to replay", total_bars);

    if total_bars == 0 {
        anyhow::bail!("No bars found in database for date filter: {:?}", replay_date);
    }

    // Notify clients we're connected
    let _ = state.tx.send(WsMessage::Connected {
        symbols: vec!["NQ".to_string()], // Will be updated from actual data
        mode: state.mode.clone(),
    });

    // Create processing state
    let processing_state = Arc::new(RwLock::new(ProcessingState::new(
        state.supabase.clone(),
        state.session_id,
        Some(state.clone()),
    )));

    // Spawn aggregation task
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

    // Fetch and stream bars in batches
    let batch_size = 1000;
    let mut offset = 0;
    let mut last_ts: Option<u64> = None;
    let mut processed = 0;

    loop {
        // Check pause state
        loop {
            let ctrl = state.replay_control.read().await;
            if !ctrl.is_paused {
                break;
            }
            drop(ctrl);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Fetch next batch
        let bars = client.fetch_bars(replay_date.as_deref(), batch_size, offset).await?;

        if bars.is_empty() {
            break;
        }

        let current_speed = state.replay_control.read().await.speed;

        for bar in &bars {
            // Parse timestamp
            let bar_ts = chrono::DateTime::parse_from_rfc3339(&bar.timestamp)
                .map(|dt| dt.timestamp_millis() as u64)
                .unwrap_or(0);

            // Update replay control
            {
                let mut ctrl = state.replay_control.write().await;
                ctrl.current_timestamp = Some(bar_ts);
            }

            // Pace based on time difference
            if let Some(prev_ts) = last_ts {
                if bar_ts > prev_ts {
                    let delay_ms = (bar_ts - prev_ts) / current_speed as u64;
                    if delay_ms > 0 && delay_ms < 5000 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            }
            last_ts = Some(bar_ts);

            // Convert bar to trades and process
            let trades = bar_to_trades(bar);
            {
                let mut pstate = processing_state.write().await;
                for trade in trades {
                    pstate.add_trade(trade);
                }
            }

            processed += 1;
        }

        offset += bars.len();

        // Log progress
        info!("Replay progress: {}/{} bars ({:.1}%)",
              processed, total_bars, (processed as f64 / total_bars as f64) * 100.0);

        if bars.len() < batch_size {
            break;
        }
    }

    info!("Database replay complete! Processed {} bars", processed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_to_trades() {
        // Would need test data
    }
}
