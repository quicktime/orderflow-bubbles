use anyhow::Result;
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::info;

use crate::processing::ProcessingState;
use crate::types::{AppState, Trade, WsMessage};

/// Demo mode: Generate realistic-looking trade data
pub async fn run_demo_stream(
    symbols: Vec<String>,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Starting demo data generator...");

    // Notify clients we're connected
    let _ = state.tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
    });

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
            let mut state = processing_state_clone.write().await;
            state.process_buffer(&tx_clone);
            state.send_volume_profile(&tx_clone);
        }
    });

    // Demo parameters
    let mut base_price = 20_100.0; // Starting NQ price
    let mut rng_state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    info!("📊 Demo mode started - generating trades for {}", symbols[0]);

    loop {
        // Generate trades at realistic intervals (10-50ms between trades)
        let sleep_ms = (xorshift(&mut rng_state) % 40) + 10;
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;

        // Random walk price
        let price_change = ((xorshift(&mut rng_state) % 5) as f64 - 2.0) * 0.25;
        base_price = (base_price + price_change).max(20_000.0).min(20_300.0);

        // Random size (1-50 contracts, weighted toward smaller sizes)
        let size_rand = xorshift(&mut rng_state) % 100;
        let size = if size_rand < 50 {
            ((xorshift(&mut rng_state) % 5) + 1) as u32 // 1-5 contracts (50%)
        } else if size_rand < 80 {
            ((xorshift(&mut rng_state) % 15) + 5) as u32 // 5-20 contracts (30%)
        } else if size_rand < 95 {
            ((xorshift(&mut rng_state) % 30) + 20) as u32 // 20-50 contracts (15%)
        } else {
            ((xorshift(&mut rng_state) % 100) + 50) as u32 // 50-150 contracts (5%)
        };

        // Random side with slight bias
        let side = if (xorshift(&mut rng_state) % 100) < 52 {
            "buy"
        } else {
            "sell"
        };

        let min_size = *state.min_size.read().await;
        if size >= min_size {
            let trade = Trade {
                symbol: symbols[0].clone(),
                price: base_price,
                size,
                side: side.to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            };

            let mut proc_state = processing_state.write().await;
            proc_state.add_trade(trade);
        }
    }
}

/// Simple xorshift PRNG for demo data
fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}
