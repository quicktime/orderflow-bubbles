use crate::bars::Bar;
use crate::impulse::ImpulseLeg;
use crate::levels::DailyLevels;
use crate::lvn::LvnLevel;
use anyhow::{Context, Result};
use arrow::array::{
    ArrayRef, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray, UInt64Array,
    BooleanArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use reqwest::Client;
use serde_json::json;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// Supabase client for data upload
pub struct SupabaseClient {
    client: Client,
    url: String,
    key: String,
}

impl SupabaseClient {
    pub fn from_env() -> Result<Self> {
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

    async fn insert_batch<T: serde::Serialize>(&self, table: &str, rows: &[T]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        // Batch in chunks of 1000
        for chunk in rows.chunks(1000) {
            let response = self.client
                .post(format!("{}/rest/v1/{}", self.url, table))
                .header("apikey", &self.key)
                .header("Authorization", format!("Bearer {}", self.key))
                .header("Content-Type", "application/json")
                .header("Prefer", "return=minimal")
                .json(chunk)
                .send()
                .await
                .context("Failed to send request to Supabase")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Supabase insert failed ({}): {}", status, text);
            }
        }

        Ok(())
    }

    pub async fn upload_bars(&self, bars: &[Bar]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct BarRow {
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

        let rows: Vec<_> = bars.iter().map(|b| BarRow {
            timestamp: b.timestamp.to_rfc3339(),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume as i64,
            buy_volume: b.buy_volume as i64,
            sell_volume: b.sell_volume as i64,
            delta: b.delta,
            trade_count: b.trade_count as i64,
            symbol: b.symbol.clone(),
        }).collect();

        self.insert_batch("replay_bars_1s", &rows).await
    }

    pub async fn upload_daily_levels(&self, levels: &[DailyLevels]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct LevelRow {
            date: String,
            symbol: String,
            pdh: f64,
            pdl: f64,
            pdc: f64,
            poc: f64,
            vah: f64,
            val: f64,
            session_high: f64,
            session_low: f64,
            session_open: f64,
            session_close: f64,
            total_volume: i64,
        }

        let rows: Vec<_> = levels.iter().map(|l| LevelRow {
            date: l.date.to_string(),
            symbol: l.symbol.clone(),
            pdh: l.pdh,
            pdl: l.pdl,
            pdc: l.pdc,
            poc: l.poc,
            vah: l.vah,
            val: l.val,
            session_high: l.session_high,
            session_low: l.session_low,
            session_open: l.session_open,
            session_close: l.session_close,
            total_volume: l.total_volume as i64,
        }).collect();

        self.insert_batch("daily_levels", &rows).await
    }

    pub async fn upload_impulse_legs(&self, legs: &[ImpulseLeg]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct LegRow {
            start_time: String,
            end_time: String,
            start_price: f64,
            end_price: f64,
            direction: String,
            symbol: String,
            date: String,
            score_total: i32,
            broke_swing: bool,
            was_fast: bool,
            uniform_candles: bool,
            volume_increased: bool,
            sufficient_size: bool,
            num_candles: i32,
            total_volume: i64,
            avg_volume_per_bar: i64,
        }

        let rows: Vec<_> = legs.iter().map(|l| LegRow {
            start_time: l.start_time.to_rfc3339(),
            end_time: l.end_time.to_rfc3339(),
            start_price: l.start_price,
            end_price: l.end_price,
            direction: format!("{:?}", l.direction),
            symbol: l.symbol.clone(),
            date: l.date.to_string(),
            score_total: l.score_total as i32,
            broke_swing: l.broke_swing,
            was_fast: l.was_fast,
            uniform_candles: l.uniform_candles,
            volume_increased: l.volume_increased,
            sufficient_size: l.sufficient_size,
            num_candles: l.num_candles as i32,
            total_volume: l.total_volume as i64,
            avg_volume_per_bar: l.avg_volume_per_bar as i64,
        }).collect();

        self.insert_batch("impulse_legs", &rows).await
    }

    pub async fn upload_lvn_levels(&self, lvns: &[LvnLevel]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct LvnRow {
            price: f64,
            volume: i64,
            avg_volume: f64,
            volume_ratio: f64,
            impulse_start_time: String,
            impulse_end_time: String,
            date: String,
            symbol: String,
        }

        let rows: Vec<_> = lvns.iter().map(|l| LvnRow {
            price: l.price,
            volume: l.volume as i64,
            avg_volume: l.avg_volume,
            volume_ratio: l.volume_ratio,
            impulse_start_time: l.impulse_start_time.to_rfc3339(),
            impulse_end_time: l.impulse_end_time.to_rfc3339(),
            date: l.date.to_string(),
            symbol: l.symbol.clone(),
        }).collect();

        self.insert_batch("lvn_levels", &rows).await
    }
}

/// Write bars to Parquet file
pub fn write_bars_parquet(bars: &[Bar], path: &Path) -> Result<()> {
    if bars.is_empty() {
        return Ok(());
    }

    let schema = Schema::new(vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::UInt64, false),
        Field::new("buy_volume", DataType::UInt64, false),
        Field::new("sell_volume", DataType::UInt64, false),
        Field::new("delta", DataType::Int64, false),
        Field::new("trade_count", DataType::UInt64, false),
        Field::new("symbol", DataType::Utf8, false),
    ]);

    let timestamps: Vec<i64> = bars.iter()
        .map(|b| b.timestamp.timestamp_micros())
        .collect();
    let opens: Vec<f64> = bars.iter().map(|b| b.open).collect();
    let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let volumes: Vec<u64> = bars.iter().map(|b| b.volume).collect();
    let buy_volumes: Vec<u64> = bars.iter().map(|b| b.buy_volume).collect();
    let sell_volumes: Vec<u64> = bars.iter().map(|b| b.sell_volume).collect();
    let deltas: Vec<i64> = bars.iter().map(|b| b.delta).collect();
    let trade_counts: Vec<u64> = bars.iter().map(|b| b.trade_count).collect();
    let symbols: Vec<&str> = bars.iter().map(|b| b.symbol.as_str()).collect();

    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(TimestampMicrosecondArray::from(timestamps)) as ArrayRef,
            Arc::new(Float64Array::from(opens)) as ArrayRef,
            Arc::new(Float64Array::from(highs)) as ArrayRef,
            Arc::new(Float64Array::from(lows)) as ArrayRef,
            Arc::new(Float64Array::from(closes)) as ArrayRef,
            Arc::new(UInt64Array::from(volumes)) as ArrayRef,
            Arc::new(UInt64Array::from(buy_volumes)) as ArrayRef,
            Arc::new(UInt64Array::from(sell_volumes)) as ArrayRef,
            Arc::new(Int64Array::from(deltas)) as ArrayRef,
            Arc::new(UInt64Array::from(trade_counts)) as ArrayRef,
            Arc::new(StringArray::from(symbols)) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

/// Write daily levels to Parquet file
pub fn write_levels_parquet(levels: &[DailyLevels], path: &Path) -> Result<()> {
    if levels.is_empty() {
        return Ok(());
    }

    let schema = Schema::new(vec![
        Field::new("date", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("pdh", DataType::Float64, false),
        Field::new("pdl", DataType::Float64, false),
        Field::new("pdc", DataType::Float64, false),
        Field::new("poc", DataType::Float64, false),
        Field::new("vah", DataType::Float64, false),
        Field::new("val", DataType::Float64, false),
        Field::new("session_high", DataType::Float64, false),
        Field::new("session_low", DataType::Float64, false),
        Field::new("session_open", DataType::Float64, false),
        Field::new("session_close", DataType::Float64, false),
        Field::new("total_volume", DataType::UInt64, false),
    ]);

    let dates: Vec<String> = levels.iter().map(|l| l.date.to_string()).collect();
    let symbols: Vec<&str> = levels.iter().map(|l| l.symbol.as_str()).collect();
    let pdhs: Vec<f64> = levels.iter().map(|l| l.pdh).collect();
    let pdls: Vec<f64> = levels.iter().map(|l| l.pdl).collect();
    let pdcs: Vec<f64> = levels.iter().map(|l| l.pdc).collect();
    let pocs: Vec<f64> = levels.iter().map(|l| l.poc).collect();
    let vahs: Vec<f64> = levels.iter().map(|l| l.vah).collect();
    let vals: Vec<f64> = levels.iter().map(|l| l.val).collect();
    let session_highs: Vec<f64> = levels.iter().map(|l| l.session_high).collect();
    let session_lows: Vec<f64> = levels.iter().map(|l| l.session_low).collect();
    let session_opens: Vec<f64> = levels.iter().map(|l| l.session_open).collect();
    let session_closes: Vec<f64> = levels.iter().map(|l| l.session_close).collect();
    let total_volumes: Vec<u64> = levels.iter().map(|l| l.total_volume).collect();

    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(StringArray::from(dates.iter().map(|s| s.as_str()).collect::<Vec<_>>())) as ArrayRef,
            Arc::new(StringArray::from(symbols)) as ArrayRef,
            Arc::new(Float64Array::from(pdhs)) as ArrayRef,
            Arc::new(Float64Array::from(pdls)) as ArrayRef,
            Arc::new(Float64Array::from(pdcs)) as ArrayRef,
            Arc::new(Float64Array::from(pocs)) as ArrayRef,
            Arc::new(Float64Array::from(vahs)) as ArrayRef,
            Arc::new(Float64Array::from(vals)) as ArrayRef,
            Arc::new(Float64Array::from(session_highs)) as ArrayRef,
            Arc::new(Float64Array::from(session_lows)) as ArrayRef,
            Arc::new(Float64Array::from(session_opens)) as ArrayRef,
            Arc::new(Float64Array::from(session_closes)) as ArrayRef,
            Arc::new(UInt64Array::from(total_volumes)) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

/// Write impulse legs to Parquet file
pub fn write_impulse_legs_parquet(legs: &[ImpulseLeg], path: &Path) -> Result<()> {
    if legs.is_empty() {
        return Ok(());
    }

    let schema = Schema::new(vec![
        Field::new("start_time", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("end_time", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("start_price", DataType::Float64, false),
        Field::new("end_price", DataType::Float64, false),
        Field::new("direction", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("score_total", DataType::Int64, false),
        Field::new("broke_swing", DataType::Boolean, false),
        Field::new("was_fast", DataType::Boolean, false),
        Field::new("uniform_candles", DataType::Boolean, false),
        Field::new("volume_increased", DataType::Boolean, false),
        Field::new("sufficient_size", DataType::Boolean, false),
        Field::new("num_candles", DataType::Int64, false),
        Field::new("total_volume", DataType::UInt64, false),
        Field::new("avg_volume_per_bar", DataType::UInt64, false),
    ]);

    let start_times: Vec<i64> = legs.iter().map(|l| l.start_time.timestamp_micros()).collect();
    let end_times: Vec<i64> = legs.iter().map(|l| l.end_time.timestamp_micros()).collect();
    let start_prices: Vec<f64> = legs.iter().map(|l| l.start_price).collect();
    let end_prices: Vec<f64> = legs.iter().map(|l| l.end_price).collect();
    let directions: Vec<String> = legs.iter().map(|l| format!("{:?}", l.direction)).collect();
    let symbols: Vec<&str> = legs.iter().map(|l| l.symbol.as_str()).collect();
    let dates: Vec<String> = legs.iter().map(|l| l.date.to_string()).collect();
    let scores: Vec<i64> = legs.iter().map(|l| l.score_total as i64).collect();
    let broke_swings: Vec<bool> = legs.iter().map(|l| l.broke_swing).collect();
    let was_fasts: Vec<bool> = legs.iter().map(|l| l.was_fast).collect();
    let uniform_candles: Vec<bool> = legs.iter().map(|l| l.uniform_candles).collect();
    let volume_increaseds: Vec<bool> = legs.iter().map(|l| l.volume_increased).collect();
    let sufficient_sizes: Vec<bool> = legs.iter().map(|l| l.sufficient_size).collect();
    let num_candles: Vec<i64> = legs.iter().map(|l| l.num_candles as i64).collect();
    let total_volumes: Vec<u64> = legs.iter().map(|l| l.total_volume).collect();
    let avg_volumes: Vec<u64> = legs.iter().map(|l| l.avg_volume_per_bar).collect();

    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(TimestampMicrosecondArray::from(start_times)) as ArrayRef,
            Arc::new(TimestampMicrosecondArray::from(end_times)) as ArrayRef,
            Arc::new(Float64Array::from(start_prices)) as ArrayRef,
            Arc::new(Float64Array::from(end_prices)) as ArrayRef,
            Arc::new(StringArray::from(directions.iter().map(|s| s.as_str()).collect::<Vec<_>>())) as ArrayRef,
            Arc::new(StringArray::from(symbols)) as ArrayRef,
            Arc::new(StringArray::from(dates.iter().map(|s| s.as_str()).collect::<Vec<_>>())) as ArrayRef,
            Arc::new(Int64Array::from(scores)) as ArrayRef,
            Arc::new(BooleanArray::from(broke_swings)) as ArrayRef,
            Arc::new(BooleanArray::from(was_fasts)) as ArrayRef,
            Arc::new(BooleanArray::from(uniform_candles)) as ArrayRef,
            Arc::new(BooleanArray::from(volume_increaseds)) as ArrayRef,
            Arc::new(BooleanArray::from(sufficient_sizes)) as ArrayRef,
            Arc::new(Int64Array::from(num_candles)) as ArrayRef,
            Arc::new(UInt64Array::from(total_volumes)) as ArrayRef,
            Arc::new(UInt64Array::from(avg_volumes)) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

/// Write LVN levels to Parquet file
pub fn write_lvn_levels_parquet(lvns: &[LvnLevel], path: &Path) -> Result<()> {
    if lvns.is_empty() {
        return Ok(());
    }

    let schema = Schema::new(vec![
        Field::new("price", DataType::Float64, false),
        Field::new("volume", DataType::UInt64, false),
        Field::new("avg_volume", DataType::Float64, false),
        Field::new("volume_ratio", DataType::Float64, false),
        Field::new("impulse_start_time", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("impulse_end_time", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("date", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
    ]);

    let prices: Vec<f64> = lvns.iter().map(|l| l.price).collect();
    let volumes: Vec<u64> = lvns.iter().map(|l| l.volume).collect();
    let avg_volumes: Vec<f64> = lvns.iter().map(|l| l.avg_volume).collect();
    let volume_ratios: Vec<f64> = lvns.iter().map(|l| l.volume_ratio).collect();
    let start_times: Vec<i64> = lvns.iter().map(|l| l.impulse_start_time.timestamp_micros()).collect();
    let end_times: Vec<i64> = lvns.iter().map(|l| l.impulse_end_time.timestamp_micros()).collect();
    let dates: Vec<String> = lvns.iter().map(|l| l.date.to_string()).collect();
    let symbols: Vec<&str> = lvns.iter().map(|l| l.symbol.as_str()).collect();

    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(Float64Array::from(prices)) as ArrayRef,
            Arc::new(UInt64Array::from(volumes)) as ArrayRef,
            Arc::new(Float64Array::from(avg_volumes)) as ArrayRef,
            Arc::new(Float64Array::from(volume_ratios)) as ArrayRef,
            Arc::new(TimestampMicrosecondArray::from(start_times)) as ArrayRef,
            Arc::new(TimestampMicrosecondArray::from(end_times)) as ArrayRef,
            Arc::new(StringArray::from(dates.iter().map(|s| s.as_str()).collect::<Vec<_>>())) as ArrayRef,
            Arc::new(StringArray::from(symbols)) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}
