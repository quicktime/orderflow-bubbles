mod trades;
mod bars;
mod levels;
mod impulse;
mod lvn;
mod supabase;
mod replay;
mod backtest;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "pipeline")]
#[command(about = "NQ futures backtesting & replay data pipeline")]
struct Args {
    #[command(subcommand)]
    command: Commands,

    /// Print verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Process trade data and export to Parquet/Supabase
    Process {
        /// Path to data directory containing .zst files
        #[arg(short, long, default_value = "data/NQ_11_23_2025-12_23_2025")]
        data_dir: PathBuf,

        /// Output directory for Parquet files
        #[arg(short, long, default_value = "output")]
        output_dir: PathBuf,

        /// Process only a specific date (YYYYMMDD format)
        #[arg(short = 'D', long)]
        date: Option<String>,

        /// Skip Supabase upload (local processing only)
        #[arg(long)]
        no_upload: bool,
    },

    /// Replay historical trades through production ProcessingState
    Replay {
        /// Path to data directory containing .zst files
        #[arg(short, long, default_value = "data/NQ_11_23_2025-12_23_2025")]
        data_dir: PathBuf,

        /// Output directory for Parquet files
        #[arg(short, long, default_value = "output")]
        output_dir: PathBuf,

        /// Process only a specific date (YYYYMMDD format)
        #[arg(short = 'D', long)]
        date: Option<String>,
    },

    /// Backtest trading strategy on historical signals
    Backtest {
        /// Path to data directory containing .zst files
        #[arg(short, long, default_value = "data/NQ_11_23_2025-12_23_2025")]
        data_dir: PathBuf,

        /// Output directory for results
        #[arg(short, long, default_value = "output")]
        output_dir: PathBuf,

        /// Process only a specific date (YYYYMMDD format)
        #[arg(short = 'D', long)]
        date: Option<String>,

        /// Stop loss in points
        #[arg(long, default_value = "10.0")]
        stop_loss: f64,

        /// Take profit in points
        #[arg(long, default_value = "20.0")]
        take_profit: f64,

        /// Maximum hold time in seconds
        #[arg(long, default_value = "300")]
        max_hold: u64,

        /// Only trade during RTH (9:30 AM - 4:00 PM ET)
        #[arg(long, default_value = "true")]
        rth_only: bool,

        /// Minimum confluence score (2-4)
        #[arg(long, default_value = "2")]
        min_confluence: u8,

        /// Only trade at key levels (POC, VAH, VAL, PDH, PDL)
        #[arg(long)]
        key_levels_only: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(if args.verbose { Level::DEBUG } else { Level::INFO })
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match args.command {
        Commands::Process { data_dir, output_dir, date, no_upload } => {
            run_process(data_dir, output_dir, date, no_upload).await?;
        }
        Commands::Replay { data_dir, output_dir, date } => {
            run_replay(data_dir, output_dir, date)?;
        }
        Commands::Backtest {
            data_dir, output_dir, date,
            stop_loss, take_profit, max_hold,
            rth_only, min_confluence, key_levels_only,
        } => {
            run_backtest(
                data_dir, output_dir, date,
                stop_loss, take_profit, max_hold,
                rth_only, min_confluence, key_levels_only,
            )?;
        }
    }

    Ok(())
}

async fn run_process(
    data_dir: PathBuf,
    output_dir: PathBuf,
    date: Option<String>,
    no_upload: bool,
) -> Result<()> {
    info!("=== PROCESS MODE ===");
    info!("Data directory: {:?}", data_dir);
    info!("Output directory: {:?}", output_dir);

    std::fs::create_dir_all(&output_dir)?;

    // Find all .zst files
    let zst_files = trades::find_zst_files(&data_dir, date.as_deref())?;
    info!("Found {} trade files to process", zst_files.len());

    if zst_files.is_empty() {
        info!("No files to process");
        return Ok(());
    }

    // Collect all data
    let mut all_bars = Vec::new();
    let mut all_daily_levels = Vec::new();
    let mut all_impulse_legs = Vec::new();
    let mut all_lvn_levels = Vec::new();

    for zst_path in &zst_files {
        info!("Processing: {:?}", zst_path);

        let trades = trades::parse_zst_trades(zst_path)?;
        info!("  Parsed {} trades", trades.len());

        if trades.is_empty() {
            continue;
        }

        let bars_1s = bars::aggregate_to_1s_bars(&trades);
        info!("  Created {} 1-second bars", bars_1s.len());

        let daily_levels = levels::compute_daily_levels(&bars_1s);
        info!("  Computed levels for {} trading days", daily_levels.len());

        let bars_1m = bars::aggregate_to_1m_bars(&bars_1s);
        info!("  Created {} 1-minute bars", bars_1m.len());

        let impulse_legs = impulse::detect_impulse_legs(&bars_1m, &daily_levels);
        info!("  Found {} valid impulse legs", impulse_legs.len());

        let lvn_levels = lvn::extract_lvns(&trades, &impulse_legs);
        info!("  Extracted {} LVN levels", lvn_levels.len());

        all_bars.extend(bars_1s);
        all_daily_levels.extend(daily_levels);
        all_impulse_legs.extend(impulse_legs);
        all_lvn_levels.extend(lvn_levels);
    }

    info!("Total: {} bars, {} daily levels, {} impulse legs, {} LVNs",
          all_bars.len(), all_daily_levels.len(),
          all_impulse_legs.len(), all_lvn_levels.len());

    // Write Parquet files
    info!("Writing Parquet files...");

    let bars_path = output_dir.join("replay_bars_1s.parquet");
    supabase::write_bars_parquet(&all_bars, &bars_path)?;
    info!("  Wrote {} bars to {:?}", all_bars.len(), bars_path);

    let levels_path = output_dir.join("daily_levels.parquet");
    supabase::write_levels_parquet(&all_daily_levels, &levels_path)?;
    info!("  Wrote {} daily levels to {:?}", all_daily_levels.len(), levels_path);

    let impulse_path = output_dir.join("impulse_legs.parquet");
    supabase::write_impulse_legs_parquet(&all_impulse_legs, &impulse_path)?;
    info!("  Wrote {} impulse legs to {:?}", all_impulse_legs.len(), impulse_path);

    let lvn_path = output_dir.join("lvn_levels.parquet");
    supabase::write_lvn_levels_parquet(&all_lvn_levels, &lvn_path)?;
    info!("  Wrote {} LVN levels to {:?}", all_lvn_levels.len(), lvn_path);

    // Upload to Supabase
    if !no_upload {
        info!("Uploading to Supabase...");
        match supabase::SupabaseClient::from_env() {
            Ok(client) => {
                client.upload_bars(&all_bars).await?;
                client.upload_daily_levels(&all_daily_levels).await?;
                client.upload_impulse_legs(&all_impulse_legs).await?;
                client.upload_lvn_levels(&all_lvn_levels).await?;
                info!("Upload complete!");
            }
            Err(e) => {
                info!("Skipping Supabase upload: {}", e);
            }
        }
    }

    info!("Process complete!");
    Ok(())
}

fn run_replay(
    data_dir: PathBuf,
    output_dir: PathBuf,
    date: Option<String>,
) -> Result<()> {
    info!("=== REPLAY MODE ===");
    info!("Replaying historical trades through production ProcessingState");
    info!("Data directory: {:?}", data_dir);

    std::fs::create_dir_all(&output_dir)?;

    // Parse trades
    let zst_files = trades::find_zst_files(&data_dir, date.as_deref())?;
    info!("Found {} trade files", zst_files.len());

    let mut all_trades = Vec::new();
    for zst_path in &zst_files {
        let trades = trades::parse_zst_trades(zst_path)?;
        info!("Parsed {} trades from {:?}", trades.len(), zst_path);
        all_trades.extend(trades);
    }

    info!("Total trades: {}", all_trades.len());

    // Replay through ProcessingState
    let signals = replay::replay_trades_for_signals(&all_trades);
    info!("Generated {} signals", signals.len());

    // Write signals to Parquet
    let signals_path = output_dir.join("signals.parquet");
    replay::write_signals_parquet(&signals, &signals_path)?;
    info!("Wrote signals to {:?}", signals_path);

    info!("Replay complete!");
    Ok(())
}

fn run_backtest(
    data_dir: PathBuf,
    output_dir: PathBuf,
    date: Option<String>,
    stop_loss: f64,
    take_profit: f64,
    max_hold: u64,
    rth_only: bool,
    min_confluence: u8,
    key_levels_only: bool,
) -> Result<()> {
    info!("=== BACKTEST MODE ===");
    info!("Running strategy backtest");
    info!("Data directory: {:?}", data_dir);

    std::fs::create_dir_all(&output_dir)?;

    // Parse trades and generate derived data
    let zst_files = trades::find_zst_files(&data_dir, date.as_deref())?;
    info!("Found {} trade files", zst_files.len());

    let mut all_trades = Vec::new();
    let mut all_bars = Vec::new();
    let mut all_daily_levels = Vec::new();

    for zst_path in &zst_files {
        let trades = trades::parse_zst_trades(zst_path)?;
        info!("Parsed {} trades from {:?}", trades.len(), zst_path);

        if !trades.is_empty() {
            let bars_1s = bars::aggregate_to_1s_bars(&trades);
            let daily_levels = levels::compute_daily_levels(&bars_1s);
            all_bars.extend(bars_1s);
            all_daily_levels.extend(daily_levels);
        }

        all_trades.extend(trades);
    }

    info!("Total: {} trades, {} bars, {} daily levels",
          all_trades.len(), all_bars.len(), all_daily_levels.len());

    // Replay through ProcessingState to get signals
    info!("Generating signals through replay...");
    let signals = replay::replay_trades_for_signals(&all_trades);
    info!("Generated {} signals", signals.len());

    // Configure backtest strategy
    let config = backtest::StrategyConfig {
        min_confluence_score: min_confluence,
        required_signals: vec![],
        stop_loss_points: stop_loss,
        take_profit_points: take_profit,
        max_hold_time_secs: max_hold,
        require_key_level: key_levels_only,
        min_strength: None,
        rth_only,
    };

    // Run backtest
    info!("Running backtest...");
    let backtester = backtest::Backtester::new(config, all_bars, all_daily_levels);
    let results = backtester.run(&signals);

    // Print results
    backtest::print_results(&results);

    // Write results to JSON
    let results_path = output_dir.join("backtest_results.json");
    let json = serde_json::to_string_pretty(&results)?;
    std::fs::write(&results_path, json)?;
    info!("Wrote results to {:?}", results_path);

    info!("Backtest complete!");
    Ok(())
}
