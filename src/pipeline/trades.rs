use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// Raw trade from Databento CSV
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub ts_event: DateTime<Utc>,
    pub price: f64,
    pub size: u64,
    pub side: Side,
    pub symbol: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

/// CSV row structure matching Databento trades schema
#[derive(Debug, Deserialize)]
struct CsvRow {
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

/// Find all .zst files in directory, optionally filtered by date
pub fn find_zst_files(data_dir: &Path, date_filter: Option<&str>) -> Result<Vec<PathBuf>> {
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
pub fn parse_zst_trades(path: &Path) -> Result<Vec<Trade>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file: {:?}", path))?;

    let decoder = zstd::stream::Decoder::new(file)
        .with_context(|| format!("Failed to create zstd decoder for: {:?}", path))?;

    let reader = BufReader::new(decoder);
    let mut csv_reader = csv::Reader::from_reader(reader);

    let mut trades = Vec::new();

    for result in csv_reader.deserialize() {
        let row: CsvRow = result.with_context(|| "Failed to parse CSV row")?;

        // Only process trade actions
        if row.action != "T" {
            continue;
        }

        let side = match row.side.as_str() {
            "B" => Side::Buy,
            "A" => Side::Sell,
            _ => continue, // Skip unknown sides
        };

        // Parse timestamp
        let ts_event = DateTime::parse_from_rfc3339(&row.ts_event)
            .with_context(|| format!("Failed to parse timestamp: {}", row.ts_event))?
            .with_timezone(&Utc);

        trades.push(Trade {
            ts_event,
            price: row.price,
            size: row.size,
            side,
            symbol: row.symbol,
        });
    }

    Ok(trades)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_zst_files() {
        let dir = Path::new("data/NQ_11_23_2025-12_23_2025");
        if dir.exists() {
            let files = find_zst_files(dir, None).unwrap();
            assert!(!files.is_empty());
        }
    }
}
