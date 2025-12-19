# FLOW - Orderflow Aggression Bubbles

Real-time orderflow visualization for NQ/ES futures. Rust backend streams from Databento, browser frontend renders bubbles.

## Architecture

```
┌─────────────────┐     ┌──────────────┐     ┌─────────────┐
│   Databento     │────▶│  Rust Server │────▶│   Browser   │
│  (CME Globex)   │     │  (WebSocket) │     │  (Canvas)   │
└─────────────────┘     └──────────────┘     └─────────────┘
```

- **Rust backend** - Connects to Databento, parses trades, broadcasts via WebSocket
- **Browser frontend** - Receives trades, renders animated bubbles on canvas

## Prerequisites

- Rust toolchain (1.70+)
- Databento account with API key
- ~$0.02/min for real-time CME data

## Quick Start

```bash
# 1. Clone/download and enter directory
cd orderflow-bubbles

# 2. Set your API key
cp .env.example .env
# Edit .env and add your Databento API key

# 3. Build and run
cargo run --release

# 4. Open browser
# http://localhost:3000
```

## Usage

### Command Line Options

```bash
# Default: NQ and ES continuous front-month
cargo run --release

# With options
cargo run --release -- \
  --api-key YOUR_KEY \
  --symbols "NQ.c.0,ES.c.0" \
  --port 3000 \
  --min-size 1
```

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `-a, --api-key` | `DATABENTO_API_KEY` | required | Databento API key |
| `-s, --symbols` | - | `NQ.c.0,ES.c.0` | Comma-separated symbols |
| `-p, --port` | - | `3000` | Web server port |
| `-m, --min-size` | - | `1` | Minimum contracts to show |

### Symbols

Databento continuous front-month notation:
- `NQ.c.0` - NQ front month (auto-rolls)
- `ES.c.0` - ES front month (auto-rolls)

Or specific contracts:
- `NQH5` - NQ March 2025
- `ESM5` - ES June 2025

### Frontend Controls

| Control | Function |
|---------|----------|
| **NQ / ES** buttons | Filter trades by symbol |
| **MIN SIZE** input | Only show trades ≥ N contracts |

### Reading the Visualization

| Element | Meaning |
|---------|---------|
| **Green bubble** | Buy aggression (trade at ask) |
| **Red bubble** | Sell aggression (trade at bid) |
| **Bubble size** | Trade size (contracts) |
| **Number inside** | Contract count (≥5 contracts) |
| **Glow effect** | Large trades (≥10 contracts) |
| **Dashed line** | Current price |

## Data Costs

| Usage | Approx Cost |
|-------|-------------|
| 1 hour NQ+ES | ~$2.40 |
| Full trading day | ~$15 |
| Your $125 credit | ~50 hours |

## Project Structure

```
orderflow-bubbles/
├── Cargo.toml          # Rust dependencies
├── src/
│   └── main.rs         # Server: Databento + WebSocket
├── frontend/
│   └── index.html      # Single-file visualization
├── .env.example        # Config template
└── README.md
```

## Building for Production

```bash
cargo build --release
./target/release/orderflow-bubbles --api-key YOUR_KEY
```

Binary is self-contained, just needs the `frontend/` directory alongside it.

## Troubleshooting

### "Failed to connect to Databento"
- Check API key is correct
- Verify you have credits remaining

### No trades appearing
- Market may be closed (CME Globex: Sun 6pm - Fri 5pm ET)
- Check MIN SIZE filter isn't too high
- Verify correct symbol is selected

### WebSocket disconnects
- Server may have crashed - check terminal output
- Databento stream may have ended - restart server

## Future Ideas

- [ ] Volume profile sidebar
- [ ] Delta ribbon
- [ ] Sound alerts for large trades  
- [ ] Trade log panel
- [ ] Historical replay via Databento batch API
- [ ] Tauri desktop app wrapper

## License

MIT
