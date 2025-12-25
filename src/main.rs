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

use orderflow_bubbles::{api, streams, supabase, types};
use streams::{run_databento_stream, run_db_replay, run_demo_stream, run_historical_replay, run_local_replay};
use supabase::{SessionRecord, SupabaseClient, UserConfig};
use types::{AppState, ClientMessage, WsMessage};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Databento API key (not required for demo/local-replay mode)
    #[arg(short, long, env = "DATABENTO_API_KEY")]
    api_key: Option<String>,

    /// Run in demo mode with simulated data
    #[arg(short, long, default_value = "false")]
    demo: bool,

    /// Run in replay mode using Databento API (requires API key)
    #[arg(short, long, default_value = "false")]
    replay: bool,

    /// Run replay from Supabase database (data uploaded by pipeline)
    #[arg(long, default_value = "false")]
    db_replay: bool,

    /// Run in local replay mode using downloaded .zst files (no API key needed)
    #[arg(long, default_value = "false")]
    local_replay: bool,

    /// Data directory for local replay (contains .zst files)
    #[arg(long, default_value = "data/NQ_11_23_2025-12_23_2025")]
    data_dir: std::path::PathBuf,

    /// Replay date (YYYY-MM-DD format, e.g., 2024-12-20)
    #[arg(long)]
    replay_date: Option<String>,

    /// Replay start time (HH:MM format in ET, e.g., 09:30) - for API replay only
    #[arg(long, default_value = "09:30")]
    replay_start: String,

    /// Replay end time (HH:MM format in ET, e.g., 16:00) - for API replay only
    #[arg(long, default_value = "16:00")]
    replay_end: String,

    /// Replay speed multiplier (1 = real-time, 10 = 10x speed)
    #[arg(long, default_value = "1")]
    replay_speed: u32,

    /// Symbols to subscribe to (comma-separated) - for live/demo mode
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

    let mode = if args.db_replay {
        "DB_REPLAY"
    } else if args.local_replay {
        "LOCAL_REPLAY"
    } else if args.replay {
        "API_REPLAY"
    } else if args.demo {
        "DEMO"
    } else {
        "LIVE"
    };
    info!("Starting Orderflow Bubbles server");
    info!("Mode: {}", mode);
    if args.db_replay {
        info!("Replaying from Supabase database");
    } else if args.local_replay {
        info!("Data dir: {:?}", args.data_dir);
    } else {
        info!("Symbols: {}", args.symbols);
    }
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

    let replay_date_clone = if args.replay || args.local_replay || args.db_replay {
        args.replay_date.clone()
    } else {
        None
    };

    let state = Arc::new(AppState {
        tx: tx.clone(),
        active_symbols: RwLock::new(symbols.iter().cloned().collect()),
        min_size: RwLock::new(min_size),
        session_id,
        supabase,
        config: RwLock::new(config),
        session_stats: RwLock::new((0.0, f64::MAX, 0)),
        mode: mode.to_lowercase(),
        replay_date: replay_date_clone,
        replay_control: RwLock::new(types::ReplayControl {
            is_paused: false,
            speed: args.replay_speed,
            current_timestamp: None,
        }),
    });

    // Spawn data streaming task (demo, replay, or live)
    let state_clone = state.clone();

    if args.db_replay {
        let replay_date = args.replay_date.clone();
        let replay_speed = args.replay_speed;

        info!("🗄️ Starting DATABASE REPLAY mode");
        info!("   Source: Supabase (replay_bars_1s table)");
        if let Some(ref date) = replay_date {
            info!("   Date filter: {}", date);
        }
        info!("   Speed: {}x", replay_speed);

        tokio::spawn(async move {
            if let Err(e) = run_db_replay(
                replay_date,
                replay_speed,
                state_clone,
            )
            .await
            {
                error!("Database replay error: {}", e);
            }
        });
    } else if args.local_replay {
        let data_dir = args.data_dir.clone();
        let replay_date = args.replay_date.clone();
        let replay_speed = args.replay_speed;

        info!("📂 Starting LOCAL REPLAY mode");
        info!("   Data dir: {:?}", data_dir);
        if let Some(ref date) = replay_date {
            info!("   Date filter: {}", date);
        }
        info!("   Speed: {}x", replay_speed);

        tokio::spawn(async move {
            if let Err(e) = run_local_replay(
                data_dir,
                replay_date,
                replay_speed,
                state_clone,
            )
            .await
            {
                error!("Local replay error: {}", e);
            }
        });
    } else if args.replay {
        let api_key = args
            .api_key
            .clone()
            .expect("API key required for API replay mode");
        let replay_date = args
            .replay_date
            .clone()
            .expect("--replay-date required for replay mode (YYYY-MM-DD)");
        let replay_start = args.replay_start.clone();
        let replay_end = args.replay_end.clone();
        let replay_speed = args.replay_speed;

        info!("⏪ Starting API REPLAY mode (Databento)");
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
            .expect("API key required for live mode (use --demo or --local-replay)");
        info!("📡 Starting LIVE mode with Databento");
        tokio::spawn(async move {
            if let Err(e) = run_databento_stream(api_key, symbols, state_clone).await {
                error!("Databento stream error: {}", e);
            }
        });
    }

    // Health check endpoint for Railway/Docker
    async fn health_check() -> &'static str {
        "OK"
    }

    // Build router with API endpoints
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/health", get(health_check))
        .route("/api/signals", get(api::get_signals))
        .route("/api/signals/export", get(api::export_signals))
        .route("/api/sessions", get(api::get_sessions))
        .route("/api/stats", get(api::get_stats))
        .nest_service("/", ServeDir::new("dist"))
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(state.clone());

    // Use 0.0.0.0 for Docker/Railway compatibility
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
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
    let welcome = WsMessage::Connected {
        symbols,
        mode: state.mode.clone(),
    };
    if let Ok(json) = serde_json::to_string(&welcome) {
        let _ = sender.send(Message::Text(json.into())).await;
    }

    // Send initial replay status
    {
        let replay_ctrl = state.replay_control.read().await;
        let status = types::ReplayStatus {
            mode: state.mode.clone(),
            is_paused: replay_ctrl.is_paused,
            speed: replay_ctrl.speed,
            replay_date: state.replay_date.clone(),
            replay_progress: None,
            current_time: replay_ctrl.current_timestamp,
        };
        if let Ok(json) = serde_json::to_string(&WsMessage::ReplayStatus(status)) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
    }

    // Spawn task to forward messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json.into())).await.is_err() {
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
                        "replay_pause" => {
                            let mut ctrl = state_clone.replay_control.write().await;
                            ctrl.is_paused = true;
                            info!("⏸️ Replay paused");
                            // Broadcast status update
                            let status = types::ReplayStatus {
                                mode: state_clone.mode.clone(),
                                is_paused: true,
                                speed: ctrl.speed,
                                replay_date: state_clone.replay_date.clone(),
                                replay_progress: None,
                                current_time: ctrl.current_timestamp,
                            };
                            let _ = state_clone.tx.send(WsMessage::ReplayStatus(status));
                        }
                        "replay_resume" => {
                            let mut ctrl = state_clone.replay_control.write().await;
                            ctrl.is_paused = false;
                            info!("▶️ Replay resumed");
                            // Broadcast status update
                            let status = types::ReplayStatus {
                                mode: state_clone.mode.clone(),
                                is_paused: false,
                                speed: ctrl.speed,
                                replay_date: state_clone.replay_date.clone(),
                                replay_progress: None,
                                current_time: ctrl.current_timestamp,
                            };
                            let _ = state_clone.tx.send(WsMessage::ReplayStatus(status));
                        }
                        "set_replay_speed" => {
                            if let Some(speed) = client_msg.speed {
                                let speed = speed.clamp(1, 100);
                                let mut ctrl = state_clone.replay_control.write().await;
                                ctrl.speed = speed;
                                info!("⏩ Replay speed set to {}x", speed);
                                // Broadcast status update
                                let status = types::ReplayStatus {
                                    mode: state_clone.mode.clone(),
                                    is_paused: ctrl.is_paused,
                                    speed,
                                    replay_date: state_clone.replay_date.clone(),
                                    replay_progress: None,
                                    current_time: ctrl.current_timestamp,
                                };
                                let _ = state_clone.tx.send(WsMessage::ReplayStatus(status));
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
