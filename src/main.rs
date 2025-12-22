use anyhow::{Context, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use databento::{
    dbn::{Record, TradeMsg},
    live::Subscription,
    LiveClient,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, RwLock};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Databento API key (not required for demo mode)
    #[arg(short, long, env = "DATABENTO_API_KEY")]
    api_key: Option<String>,

    /// Run in demo mode with simulated data
    #[arg(short, long, default_value = "false")]
    demo: bool,

    /// Symbols to subscribe to (comma-separated)
    #[arg(short = 's', long, default_value = "NQ.c.0,ES.c.0")]
    symbols: String,

    /// Port to run the web server on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Minimum trade size to process
    #[arg(short = 'f', long, default_value = "1")]
    min_size: u32,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    Bubble(Bubble),
    CVDPoint(CVDPoint),
    VolumeProfile { levels: Vec<VolumeProfileLevel> },
    Connected { symbols: Vec<String> },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub action: String,
    pub symbol: Option<String>,
    pub min_size: Option<u32>,
}

struct AppState {
    tx: broadcast::Sender<WsMessage>,
    active_symbols: RwLock<HashSet<String>>,
    min_size: RwLock<u32>,
}

// Processing state for aggregation
struct ProcessingState {
    trade_buffer: Vec<Trade>,
    bubble_counter: u64,
    cvd: i64,
    volume_profile: HashMap<i64, VolumeProfileLevel>, // Key = price * 4 (for 0.25 tick size)
    total_buy_volume: u64,
    total_sell_volume: u64,
}

impl ProcessingState {
    fn new() -> Self {
        Self {
            trade_buffer: Vec::new(),
            bubble_counter: 0,
            cvd: 0,
            volume_profile: HashMap::new(),
            total_buy_volume: 0,
            total_sell_volume: 0,
        }
    }

    fn add_trade(&mut self, trade: Trade) {
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

    fn process_buffer(&mut self, tx: &broadcast::Sender<WsMessage>) {
        if self.trade_buffer.is_empty() {
            return;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

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
            self.trade_buffer.iter().map(|t| t.price).sum::<f64>()
                / self.trade_buffer.len() as f64
        };

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

    fn send_volume_profile(&self, tx: &broadcast::Sender<WsMessage>) {
        let levels: Vec<VolumeProfileLevel> = self.volume_profile.values().cloned().collect();
        let _ = tx.send(WsMessage::VolumeProfile { levels });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("orderflow_bubbles=info".parse().unwrap())
                .add_directive("databento=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    info!("Starting Orderflow Bubbles server");
    info!("Mode: {}", if args.demo { "DEMO" } else { "LIVE" });
    info!("Symbols: {}", args.symbols);
    info!("Port: {}", args.port);
    info!("Min size filter: {}", args.min_size);

    // Create broadcast channel for processed data
    let (tx, _rx) = broadcast::channel::<WsMessage>(1000);

    let symbols: Vec<String> = args
        .symbols
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let state = Arc::new(AppState {
        tx: tx.clone(),
        active_symbols: RwLock::new(symbols.iter().cloned().collect()),
        min_size: RwLock::new(args.min_size),
    });

    // Spawn data streaming task (either demo or live)
    let tx_clone = tx.clone();
    let state_clone = state.clone();

    if args.demo {
        info!("🎮 Starting DEMO mode with simulated data");
        tokio::spawn(async move {
            if let Err(e) = run_demo_stream(symbols, tx_clone, state_clone).await {
                error!("Demo stream error: {}", e);
            }
        });
    } else {
        let api_key = args.api_key.clone().expect("API key required for live mode (use --demo for demo mode)");
        info!("📡 Starting LIVE mode with Databento");
        tokio::spawn(async move {
            if let Err(e) = run_databento_stream(api_key, symbols, tx_clone, state_clone).await {
                error!("Databento stream error: {}", e);
            }
        });
    }

    // Build router
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .nest_service("/", ServeDir::new("dist"))
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    info!("Server running at http://{}", addr);
    info!("WebSocket endpoint: ws://localhost:{}/ws", args.port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// Demo mode: Generate realistic-looking trade data
async fn run_demo_stream(
    symbols: Vec<String>,
    tx: broadcast::Sender<WsMessage>,
    state: Arc<AppState>,
) -> Result<()> {
    info!("Starting demo data generator...");

    // Notify clients we're connected
    let _ = tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
    });

    // Create processing state
    let processing_state = Arc::new(RwLock::new(ProcessingState::new()));

    // Spawn 1-second aggregation task
    let processing_state_clone = processing_state.clone();
    let tx_clone = tx.clone();
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

// Simple xorshift PRNG for demo data
fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

async fn run_databento_stream(
    api_key: String,
    symbols: Vec<String>,
    tx: broadcast::Sender<WsMessage>,
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
        .schema(databento::dbn::Schema::Trades)
        .stype_in(databento::dbn::SType::RawSymbol)
        .build();

    client
        .subscribe(&subscription)
        .await
        .context("Failed to subscribe")?;

    info!("Subscribed to: {:?}", symbols);

    // Notify clients we're connected
    let _ = tx.send(WsMessage::Connected {
        symbols: symbols.clone(),
    });

    // Start streaming
    client.start().await.context("Failed to start stream")?;

    // Create processing state
    let processing_state = Arc::new(RwLock::new(ProcessingState::new()));

    // Spawn 1-second aggregation task
    let processing_state_clone = processing_state.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut state = processing_state_clone.write().await;
            state.process_buffer(&tx_clone);

            // Send volume profile every second
            state.send_volume_profile(&tx_clone);
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
                let mut state = processing_state.write().await;
                state.add_trade(trade_msg);
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

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // Send current state to new client
    let symbols: Vec<String> = state.active_symbols.read().await.iter().cloned().collect();
    let welcome = WsMessage::Connected { symbols };
    if let Ok(json) = serde_json::to_string(&welcome) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Spawn task to forward messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages from client
    let state_clone = state.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg.action.as_str() {
                        "set_min_size" => {
                            if let Some(size) = client_msg.min_size {
                                *state_clone.min_size.write().await = size;
                                info!("Min size filter set to: {}", size);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    info!("WebSocket client disconnected");
}
