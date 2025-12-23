mod api;
mod processing;
mod streams;
mod supabase;
mod types;

use anyhow::Result;
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
use futures::{SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};
use tracing::{error, info};

use streams::{run_databento_stream, run_demo_stream, run_historical_replay};
use supabase::{SessionRecord, SupabaseClient, UserConfig};
use types::{AppState, ClientMessage, WsMessage};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Databento API key (not required for demo mode)
    #[arg(short, long, env = "DATABENTO_API_KEY")]
    api_key: Option<String>,

    /// Run in demo mode with simulated data
    #[arg(short, long, default_value = "false")]
    demo: bool,

    /// Run in replay mode with historical data
    #[arg(short, long, default_value = "false")]
    replay: bool,

    /// Replay date (YYYY-MM-DD format, e.g., 2024-12-20)
    #[arg(long)]
    replay_date: Option<String>,

    /// Replay start time (HH:MM format in ET, e.g., 09:30)
    #[arg(long, default_value = "09:30")]
    replay_start: String,

    /// Replay end time (HH:MM format in ET, e.g., 16:00)
    #[arg(long, default_value = "16:00")]
    replay_end: String,

    /// Replay speed multiplier (1 = real-time, 10 = 10x speed)
    #[arg(long, default_value = "1")]
    replay_speed: u32,

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

    let mode = if args.replay {
        "REPLAY"
    } else if args.demo {
        "DEMO"
    } else {
        "LIVE"
    };
    info!("Starting Orderflow Bubbles server");
    info!("Mode: {}", mode);
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

    // Initialize Supabase client (optional - works without it)
    let (supabase, session_id, config) = match SupabaseClient::from_env() {
        Some(client) => {
            info!("📊 Supabase connected - signals will be persisted");

            // Load user config from Supabase
            let config = match client.get_config().await {
                Ok(cfg) => {
                    info!("📊 Config loaded: min_size={}, sound={}", cfg.min_size, cfg.sound_enabled);
                    cfg
                }
                Err(e) => {
                    info!("📊 Using default config (load failed: {})", e);
                    UserConfig::default()
                }
            };

            let session = SessionRecord {
                id: None,
                mode: mode.to_lowercase(),
                symbols: symbols.clone(),
                session_high: None,
                session_low: None,
                total_volume: None,
            };
            match client.insert_session(&session).await {
                Ok(id) => {
                    info!("📊 Session created: {}", id);
                    (Some(client), Some(id), config)
                }
                Err(e) => {
                    error!("Failed to create session in Supabase: {}", e);
                    (Some(client), None, config)
                }
            }
        }
        None => {
            info!("📊 Supabase not configured - signals will not be persisted");
            info!("   Set SUPABASE_URL and SUPABASE_ANON_KEY to enable persistence");
            (None, None, UserConfig::default())
        }
    };

    // Use CLI arg if provided, otherwise use config value
    let min_size = if args.min_size != 1 {
        args.min_size // CLI override
    } else {
        config.min_size // From Supabase config
    };

    let state = Arc::new(AppState {
        tx: tx.clone(),
        active_symbols: RwLock::new(symbols.iter().cloned().collect()),
        min_size: RwLock::new(min_size),
        session_id,
        supabase,
        config: RwLock::new(config),
        session_stats: RwLock::new((0.0, f64::MAX, 0)),
    });

    // Spawn data streaming task (demo, replay, or live)
    let state_clone = state.clone();

    if args.replay {
        let api_key = args
            .api_key
            .clone()
            .expect("API key required for replay mode");
        let replay_date = args
            .replay_date
            .clone()
            .expect("--replay-date required for replay mode (YYYY-MM-DD)");
        let replay_start = args.replay_start.clone();
        let replay_end = args.replay_end.clone();
        let replay_speed = args.replay_speed;

        info!("⏪ Starting REPLAY mode");
        info!("   Date: {}", replay_date);
        info!("   Time: {} - {} ET", replay_start, replay_end);
        info!("   Speed: {}x", replay_speed);

        tokio::spawn(async move {
            if let Err(e) = run_historical_replay(
                api_key,
                symbols,
                replay_date,
                replay_start,
                replay_end,
                replay_speed,
                state_clone,
            )
            .await
            {
                error!("Replay error: {}", e);
            }
        });
    } else if args.demo {
        info!("🎮 Starting DEMO mode with simulated data");
        tokio::spawn(async move {
            if let Err(e) = run_demo_stream(symbols, state_clone).await {
                error!("Demo stream error: {}", e);
            }
        });
    } else {
        let api_key = args
            .api_key
            .clone()
            .expect("API key required for live mode (use --demo for demo mode)");
        info!("📡 Starting LIVE mode with Databento");
        tokio::spawn(async move {
            if let Err(e) = run_databento_stream(api_key, symbols, state_clone).await {
                error!("Databento stream error: {}", e);
            }
        });
    }

    // Build router with API endpoints
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/signals", get(api::get_signals))
        .route("/api/signals/export", get(api::export_signals))
        .route("/api/sessions", get(api::get_sessions))
        .route("/api/stats", get(api::get_stats))
        .nest_service("/", ServeDir::new("dist"))
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(state.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    info!("Server running at http://{}", addr);
    info!("WebSocket endpoint: ws://localhost:{}/ws", args.port);
    info!("API endpoints: /api/signals, /api/sessions, /api/stats");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state))
        .await?;

    Ok(())
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

                                // Persist config change to Supabase
                                if let Some(ref supabase) = state_clone.supabase {
                                    let mut config = state_clone.config.write().await;
                                    config.min_size = size;
                                    let config_clone = config.clone();
                                    let supabase_clone = supabase.clone();
                                    // Fire and forget - don't block on persistence
                                    tokio::spawn(async move {
                                        if let Err(e) = supabase_clone.set_config(&config_clone).await {
                                            error!("Failed to persist config: {}", e);
                                        } else {
                                            info!("📊 Config persisted to Supabase");
                                        }
                                    });
                                }
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

/// Graceful shutdown handler - finalize session on Ctrl+C
async fn shutdown_signal(state: Arc<AppState>) {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    info!("🛑 Shutdown signal received, finalizing session...");

    // Finalize session in Supabase with actual stats
    if let (Some(ref supabase), Some(session_id)) = (&state.supabase, state.session_id) {
        let (high, low, volume) = *state.session_stats.read().await;
        // Normalize low if it was never set (still at f64::MAX)
        let low = if low == f64::MAX { high } else { low };

        info!("📊 Session stats: high={:.2}, low={:.2}, volume={}", high, low, volume);

        if let Err(e) = supabase.update_session(session_id, high, low, volume).await {
            error!("Failed to finalize session: {}", e);
        } else {
            info!("📊 Session finalized: {}", session_id);
        }
    }
}
