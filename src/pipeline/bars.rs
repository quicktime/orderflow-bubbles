use crate::trades::{Side, Trade};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// OHLCV bar with delta (buy volume - sell volume)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub timestamp: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub buy_volume: u64,
    pub sell_volume: u64,
    pub delta: i64,
    pub trade_count: u64,
    pub symbol: String,
}

impl Bar {
    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    pub fn body_size(&self) -> f64 {
        (self.close - self.open).abs()
    }

    pub fn range(&self) -> f64 {
        self.high - self.low
    }
}

/// Aggregate trades to 1-second bars
pub fn aggregate_to_1s_bars(trades: &[Trade]) -> Vec<Bar> {
    if trades.is_empty() {
        return Vec::new();
    }

    // Group trades by second
    let mut bars_map: BTreeMap<DateTime<Utc>, BarBuilder> = BTreeMap::new();

    for trade in trades {
        let second_ts = trade.ts_event
            .with_nanosecond(0)
            .unwrap();

        let builder = bars_map.entry(second_ts).or_insert_with(|| {
            BarBuilder::new(second_ts, trade.symbol.clone())
        });

        builder.add_trade(trade);
    }

    bars_map.into_values().map(|b| b.build()).collect()
}

/// Aggregate 1-second bars to 1-minute bars
pub fn aggregate_to_1m_bars(bars_1s: &[Bar]) -> Vec<Bar> {
    if bars_1s.is_empty() {
        return Vec::new();
    }

    let mut bars_map: BTreeMap<DateTime<Utc>, BarBuilder> = BTreeMap::new();

    for bar in bars_1s {
        let minute_ts = bar.timestamp
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let builder = bars_map.entry(minute_ts).or_insert_with(|| {
            BarBuilder::new(minute_ts, bar.symbol.clone())
        });

        builder.add_bar(bar);
    }

    bars_map.into_values().map(|b| b.build()).collect()
}

/// Helper to accumulate bar data
struct BarBuilder {
    timestamp: DateTime<Utc>,
    symbol: String,
    open: Option<f64>,
    high: f64,
    low: f64,
    close: f64,
    buy_volume: u64,
    sell_volume: u64,
    trade_count: u64,
    first_ts: Option<DateTime<Utc>>,
}

impl BarBuilder {
    fn new(timestamp: DateTime<Utc>, symbol: String) -> Self {
        Self {
            timestamp,
            symbol,
            open: None,
            high: f64::MIN,
            low: f64::MAX,
            close: 0.0,
            buy_volume: 0,
            sell_volume: 0,
            trade_count: 0,
            first_ts: None,
        }
    }

    fn add_trade(&mut self, trade: &Trade) {
        if self.open.is_none() || self.first_ts.map_or(true, |ts| trade.ts_event < ts) {
            self.open = Some(trade.price);
            self.first_ts = Some(trade.ts_event);
        }

        self.high = self.high.max(trade.price);
        self.low = self.low.min(trade.price);
        self.close = trade.price;

        match trade.side {
            Side::Buy => self.buy_volume += trade.size,
            Side::Sell => self.sell_volume += trade.size,
        }

        self.trade_count += 1;
    }

    fn add_bar(&mut self, bar: &Bar) {
        if self.open.is_none() || self.first_ts.map_or(true, |ts| bar.timestamp < ts) {
            self.open = Some(bar.open);
            self.first_ts = Some(bar.timestamp);
        }

        self.high = self.high.max(bar.high);
        self.low = self.low.min(bar.low);
        self.close = bar.close;

        self.buy_volume += bar.buy_volume;
        self.sell_volume += bar.sell_volume;
        self.trade_count += bar.trade_count;
    }

    fn build(self) -> Bar {
        let volume = self.buy_volume + self.sell_volume;
        let delta = self.buy_volume as i64 - self.sell_volume as i64;

        Bar {
            timestamp: self.timestamp,
            open: self.open.unwrap_or(self.close),
            high: if self.high == f64::MIN { self.close } else { self.high },
            low: if self.low == f64::MAX { self.close } else { self.low },
            close: self.close,
            volume,
            buy_volume: self.buy_volume,
            sell_volume: self.sell_volume,
            delta,
            trade_count: self.trade_count,
            symbol: self.symbol,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_aggregation() {
        let ts = Utc::now();
        let trades = vec![
            Trade {
                ts_event: ts,
                price: 100.0,
                size: 5,
                side: Side::Buy,
                symbol: "NQH6".to_string(),
            },
            Trade {
                ts_event: ts + Duration::milliseconds(100),
                price: 101.0,
                size: 3,
                side: Side::Sell,
                symbol: "NQH6".to_string(),
            },
        ];

        let bars = aggregate_to_1s_bars(&trades);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, 100.0);
        assert_eq!(bars[0].close, 101.0);
        assert_eq!(bars[0].high, 101.0);
        assert_eq!(bars[0].low, 100.0);
        assert_eq!(bars[0].buy_volume, 5);
        assert_eq!(bars[0].sell_volume, 3);
        assert_eq!(bars[0].delta, 2);
    }
}
