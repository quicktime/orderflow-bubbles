use anyhow::{Context, Result};
use databento::{
    dbn::{Record, Schema, SType, TradeMsg},
    live::Subscription,
    LiveClient,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::processing::ProcessingState;
use crate::types::{AppState, Trade, WsMessage};

/// Live mode: Stream real-time data from Databento
pub async fn run_databento_stream(
    api_key: String,
    symbols: Vec<String>,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Connecting to Databento...");

    let mut client = LiveClient::builder()
        .key(api_key)?
        .dataset("GLBX.MDP3")
        .build()
        .await
        .context("Failed to connect to Databento")?;

    info!("Connected to Databento");

    // Subscribe to symbols
    let subscription = Subscription::builder()
        .symbols(symbols.clone())
        .schema(Schema::Trades)
        .stype_in(SType::RawSymbol)
        .build();

    client
        .subscribe(&subscription)
        .await
        .context("Failed to subscribe")?;

    info!("Subscribed to: {:?}", symbols);

    // Notify clients we're connected
    let _ = state.tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
    });

    // Start streaming
    client.start().await.context("Failed to start stream")?;

    // Create processing state with Supabase persistence and AppState for stats sync
    let processing_state = Arc::new(RwLock::new(ProcessingState::new(
        state.supabase.clone(),
        state.session_id,
        Some(state.clone()),
    )));

    // Spawn 1-second aggregation task
    let processing_state_clone = processing_state.clone();
    let tx_clone = state.tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut pstate = processing_state_clone.write().await;
            pstate.process_buffer(&tx_clone);

            // Send volume profile every second
            pstate.send_volume_profile(&tx_clone);
        }
    });

    // Process incoming records
    while let Some(record) = client.next_record().await? {
        if let Some(trade) = record.get::<TradeMsg>() {
            let min_size = *state.min_size.read().await;

            if trade.size >= min_size {
                // Determine buy/sell from aggressor side
                // 'A' = Ask side (buyer aggressor), 'B' = Bid side (seller aggressor)
                let side = match trade.side as u8 {
                    b'A' | b'a' => "buy",
                    b'B' | b'b' => "sell",
                    _ => "buy", // Default
                };

                // Get symbol from instrument ID
                let symbol = get_symbol_from_record(&record, &symbols);

                let trade_msg = Trade {
                    symbol,
                    price: trade.price as f64 / 1_000_000_000.0, // Fixed-point conversion
                    size: trade.size,
                    side: side.to_string(),
                    timestamp: trade.hd.ts_event / 1_000_000, // Nanos to millis
                };

                // Add trade to processing buffer
                let mut pstate = processing_state.write().await;
                pstate.add_trade(trade_msg);
            }
        }
    }

    warn!("Databento stream ended");
    Ok(())
}

fn get_symbol_from_record(_record: &dyn Record, symbols: &[String]) -> String {
    // For simplicity, if we only have one symbol, return it
    // In production, you'd map instrument_id to symbol
    if symbols.len() == 1 {
        return symbols[0].clone();
    }

    // Default to first symbol - proper implementation would use symbol mapping
    symbols
        .first()
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".to_string())
}
