use anyhow::{Context, Result};
use databento::{
    dbn::{Dataset, SType, Schema, TradeMsg},
    historical::timeseries::GetRangeParams,
    HistoricalClient,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::info;

use crate::processing::ProcessingState;
use crate::types::{AppState, Trade, WsMessage};

/// Historical replay mode: fetch trades from Databento and replay at specified speed
pub async fn run_historical_replay(
    api_key: String,
    symbols: Vec<String>,
    replay_date: String,
    replay_start: String,
    replay_end: String,
    replay_speed: u32,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Starting historical replay...");

    // Parse date (YYYY-MM-DD)
    let date_parts: Vec<&str> = replay_date.split('-').collect();
    if date_parts.len() != 3 {
        anyhow::bail!("Invalid date format. Use YYYY-MM-DD");
    }
    let year: i32 = date_parts[0].parse().context("Invalid year")?;
    let month: u8 = date_parts[1].parse().context("Invalid month")?;
    let day: u8 = date_parts[2].parse().context("Invalid day")?;

    let date = time::Date::from_calendar_date(
        year,
        time::Month::try_from(month).context("Invalid month")?,
        day,
    )
    .context("Invalid date")?;

    // Parse start/end times (HH:MM)
    let start_parts: Vec<&str> = replay_start.split(':').collect();
    let end_parts: Vec<&str> = replay_end.split(':').collect();

    let start_hour: u8 = start_parts[0].parse().context("Invalid start hour")?;
    let start_min: u8 = start_parts[1].parse().context("Invalid start minute")?;
    let end_hour: u8 = end_parts[0].parse().context("Invalid end hour")?;
    let end_min: u8 = end_parts[1].parse().context("Invalid end minute")?;

    let start_time =
        time::Time::from_hms(start_hour, start_min, 0).context("Invalid start time")?;
    let end_time = time::Time::from_hms(end_hour, end_min, 0).context("Invalid end time")?;

    // ET offset (EST = -5, EDT = -4) - approximate with -5 for now
    let et_offset = time::UtcOffset::from_hms(-5, 0, 0).unwrap();

    let start_dt = time::PrimitiveDateTime::new(date, start_time).assume_offset(et_offset);
    let end_dt = time::PrimitiveDateTime::new(date, end_time).assume_offset(et_offset);

    info!(
        "Fetching historical data from {} to {} ET",
        replay_start, replay_end
    );

    // Build historical client
    let mut client = HistoricalClient::builder().key(api_key)?.build()?;

    // Request the data
    let params = GetRangeParams::builder()
        .dataset(Dataset::GlbxMdp3)
        .date_time_range((start_dt, end_dt))
        .symbols(symbols.clone())
        .stype_in(SType::RawSymbol)
        .schema(Schema::Trades)
        .build();

    info!("Requesting historical trades for {:?}...", symbols);
    let mut decoder = client
        .timeseries()
        .get_range(&params)
        .await
        .context("Failed to fetch historical data")?;

    info!("Historical data received, starting replay...");

    // Notify clients we're connected (in replay mode)
    let _ = state.tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
    });

    // Create processing state with Supabase persistence
    let processing_state = Arc::new(RwLock::new(ProcessingState::new(
        state.supabase.clone(),
        state.session_id,
    )));

    // Spawn aggregation task (but with speed multiplier)
    let processing_state_clone = processing_state.clone();
    let tx_clone = state.tx.clone();
    let speed = replay_speed;
    tokio::spawn(async move {
        // Interval is shortened by speed multiplier
        let interval_ms = 1000 / speed as u64;
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms.max(50)));
        loop {
            interval.tick().await;
            let mut pstate = processing_state_clone.write().await;
            pstate.process_buffer(&tx_clone);
            pstate.send_volume_profile(&tx_clone);
        }
    });

    // Get symbol map for the date
    let symbol_map = decoder.metadata().symbol_map_for_date(date)?;

    // Track timestamps for pacing
    let mut last_trade_ts: Option<u64> = None;

    // Process each trade
    while let Some(trade_msg) = decoder.decode_record::<TradeMsg>().await? {
        let trade_ts = trade_msg.hd.ts_event / 1_000_000; // nanoseconds to milliseconds

        // Pace the trades according to their original timing (adjusted by speed)
        if let Some(last_ts) = last_trade_ts {
            if trade_ts > last_ts {
                let delay_ms = (trade_ts - last_ts) / replay_speed as u64;
                if delay_ms > 0 && delay_ms < 5000 {
                    // Cap at 5 seconds to skip gaps
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
        last_trade_ts = Some(trade_ts);

        // Get symbol name
        let symbol = symbol_map
            .get(trade_msg.hd.instrument_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("ID:{}", trade_msg.hd.instrument_id));

        // Determine side from trade action (action is i8, convert to u8 for char)
        let action_char = trade_msg.action as u8 as char;
        let side_char = trade_msg.side as u8 as char;
        let side = match action_char {
            'B' => "buy",
            'S' | 'A' => "sell",
            _ => {
                if side_char == 'B' {
                    "buy"
                } else {
                    "sell"
                }
            }
        };

        let min_size = *state.min_size.read().await;
        let size = trade_msg.size;

        if size >= min_size {
            let trade = Trade {
                symbol: symbol.clone(),
                price: trade_msg.price as f64 / 1_000_000_000.0, // Fixed-point to float
                size,
                side: side.to_string(),
                timestamp: trade_ts,
            };

            let mut proc_state = processing_state.write().await;
            proc_state.add_trade(trade);
        }
    }

    info!("Replay complete!");
    Ok(())
}
