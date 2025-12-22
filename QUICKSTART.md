# FLOW - 5-Minute Quickstart

Get FLOW running in demo mode in under 5 minutes. No Databento account needed!

## Prerequisites

- **Rust** (install: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Node.js** (v18+)

## Steps

### 1. Install Dependencies

```bash
# Install Node dependencies
npm install

# Build frontend
npm run build
```

### 2. Run in Demo Mode

```bash
# Build and run Rust backend with simulated data
cargo run --release -- --demo
```

You should see:
```
Starting Orderflow Bubbles server
Mode: DEMO
Symbols: NQ.c.0,ES.c.0
Port: 8080
🎮 Starting DEMO mode with simulated data
📊 Demo mode started - generating trades for NQ.c.0
Server running at http://127.0.0.1:8080
```

### 3. Open Browser

Navigate to: **http://localhost:8080**

You should see:
- Green/red bubbles flowing across the screen (orderflow aggression)
- CVD (Cumulative Volume Delta) updating in header
- Volume Profile on the left side
- POC, VAH, VAL lines

### 4. Interact

**Keyboard Shortcuts:**
- Press **?** to see all shortcuts
- **Space** - Pause/Resume
- **R** - Reset CVD
- **C** - Clear bubbles
- **S** - Export screenshot
- **Click** on a bubble to see details

---

## What's Happening?

The demo mode:
1. Generates realistic NQ futures trades (10-50ms intervals)
2. Random walk price movement (20,000 - 20,300)
3. Weighted trade sizes (mostly 1-5 contracts, occasionally 50-150)
4. Slight buy bias (52/48) to show CVD movement
5. Full processing pipeline (same as live data!)

---

## Next Steps

### Want to See Live Data?

1. Sign up at [databento.com](https://databento.com/)
2. Get your API key
3. Create `.env` file:
   ```bash
   cp .env.example .env
   nano .env  # Add your API key
   ```
4. Run live mode:
   ```bash
   cargo run --release
   ```

### Customize Demo Mode

```bash
# Run on different port
cargo run --release -- --demo --port 3000

# Filter smaller trades
cargo run --release -- --demo --min-size 10
```

---

## Troubleshooting

### "Connection refused"

The Rust server isn't running. Make sure you see:
```
Server running at http://127.0.0.1:8080
```

### No bubbles appearing

1. Check the terminal - you should see logs like:
   ```
   Created bubble: BUY aggression=15 (65% imbalance) COLORED
   ```
2. Try refreshing the page
3. Press **R** to reset CVD

### Frontend not loading

Make sure you ran `npm run build` first. The `dist/` folder should exist.

---

## Performance Tips

Demo mode generates ~20-100 trades/second by default. If you want more action:

Edit `src/main.rs` line 403:
```rust
// Change from 10-50ms to 1-10ms for more trades
let sleep_ms = (xorshift(&mut rng_state) % 9) + 1;
```

Rebuild:
```bash
cargo build --release
cargo run --release -- --demo
```

---

## Ready to Trade?

See [SETUP.md](SETUP.md) for full documentation including:
- Live Databento integration
- Production deployment
- Docker setup
- Advanced configuration

**Enjoy trading with FLOW!** 📊🚀
