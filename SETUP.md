# FLOW - Orderflow Bubbles Setup Guide

## Architecture

FLOW now uses a **Rust backend** for high-performance data processing:

```
Databento API → Rust Backend → WebSocket → React Frontend
                (processing)              (visualization)
```

### What Rust Does:
- Connects to Databento for real-time NQ/ES market data
- 1-second trade aggregation
- Bubble creation with imbalance calculations
- CVD (Cumulative Volume Delta) tracking
- Volume Profile calculations (POC, VAH, VAL, LVN)
- WebSocket server for frontend communication

### What React Does:
- Canvas visualization
- User interactions (clicks, keyboard shortcuts)
- UI state management

---

## Prerequisites

1. **Rust** (latest stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Node.js** (v18+)
   ```bash
   # Install via nvm or download from nodejs.org
   ```

3. **Databento API Key**
   - Sign up at [databento.com](https://databento.com/)
   - Get your API key from the portal

---

## Setup

### 1. Clone and Install Dependencies

```bash
# Install Rust dependencies
cargo build --release

# Install Node dependencies
npm install
```

### 2. Configure Environment

```bash
# Copy the example env file
cp .env.example .env

# Edit .env and add your Databento API key
nano .env
```

Your `.env` should look like:
```
DATABENTO_API_KEY=db-your-actual-api-key-here
```

### 3. Build Frontend

```bash
npm run build
```

This creates the `dist/` folder that the Rust server will serve.

---

## Running FLOW

### Demo Mode (No Databento Required!) 🎮

Perfect for testing without paying for Databento:

```bash
cargo run --release -- --demo
```

This generates realistic simulated NQ futures data and processes it through the same pipeline as live data.

### Live Mode (With Databento)

```bash
cargo run --release -- --api-key $DATABENTO_API_KEY
```

Or use the environment variable from `.env`:
```bash
cargo run --release
```

**Options:**
- `--demo` - Run in demo mode with simulated data (no API key needed)
- `--port <PORT>` - Server port (default: 8080)
- `--symbols <SYMBOLS>` - Comma-separated symbols (default: "NQ.c.0,ES.c.0")
- `--min-size <SIZE>` - Minimum trade size to process (default: 1)

Examples:
```bash
# Demo mode on custom port
cargo run --release -- --demo --port 3000

# Live mode with specific symbol
cargo run --release -- \
  --api-key $DATABENTO_API_KEY \
  --symbols "NQ.c.0" \
  --min-size 5
```

### Access the App

Open your browser to:
```
http://localhost:8080
```

The React frontend will automatically connect to the Rust WebSocket backend.

---

## Development Workflow

### Frontend Development (React + Vite)

For hot-reload during frontend development:

1. Start the Rust backend on port 8080:
   ```bash
   cargo run --release
   ```

2. In a separate terminal, start Vite dev server:
   ```bash
   npm run dev
   ```

3. Open `http://localhost:3000` (Vite dev server)

4. Update `src/websocket.ts` to point to Rust backend:
   ```typescript
   constructor(url: string = 'ws://localhost:8080/ws') {
   ```

When done, rebuild the frontend:
```bash
npm run build
cargo run --release
```

### Backend Development (Rust)

After making changes to `src/main.rs`:

```bash
cargo build --release
cargo run --release
```

---

## Keyboard Shortcuts

Once running, press **?** to see all keyboard shortcuts:

- **Space** - Pause/Resume animation
- **R** - Reset CVD to zero
- **C** - Clear all bubbles
- **M** - Mute/Unmute alerts
- **S** - Export screenshot
- **Click** - Show bubble details
- **Esc** - Close help modal

---

## Troubleshooting

### "Connection refused" error

Make sure the Rust backend is running:
```bash
cargo run --release
```

Check the logs for:
```
Server running at http://127.0.0.1:8080
WebSocket endpoint: ws://localhost:8080/ws
```

### Databento connection issues

1. Verify your API key is correct in `.env`
2. Check Databento status: https://status.databento.com/
3. Ensure you have an active subscription for `GlbxMdp3` dataset
4. Check Rust backend logs for detailed error messages

### No bubbles appearing

1. Check if market is open (NQ/ES futures trading hours)
2. Adjust `--min-size` parameter to a lower value
3. Check the Rust backend logs for trade processing messages

### Frontend not loading

1. Ensure `dist/` folder exists: `npm run build`
2. Check Rust backend is serving from `dist/`: look for "nest_service" in main.rs

---

## Architecture Details

### Message Types (WebSocket)

The Rust backend sends these message types to the frontend:

1. **Bubble** - Aggregated 1-second bubble
   ```json
   {
     "type": "Bubble",
     "id": "bubble-123",
     "price": 20123.50,
     "size": 150,
     "side": "buy",
     "timestamp": 1703001234567,
     "x": 0.92,
     "opacity": 1.0,
     "isSignificantImbalance": true
   }
   ```

2. **CVDPoint** - CVD value update
   ```json
   {
     "type": "CVDPoint",
     "timestamp": 1703001234567,
     "value": 4500,
     "x": 0.92
   }
   ```

3. **VolumeProfile** - Volume profile levels
   ```json
   {
     "type": "VolumeProfile",
     "levels": [
       {
         "price": 20123.50,
         "buyVolume": 500,
         "sellVolume": 300,
         "totalVolume": 800
       }
     ]
   }
   ```

4. **Connected** - Connection confirmation
   ```json
   {
     "type": "Connected",
     "symbols": ["NQ.c.0", "ES.c.0"]
   }
   ```

---

## Production Deployment

### Docker (Recommended)

Create a `Dockerfile`:
```dockerfile
FROM rust:latest as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates
COPY --from=builder /app/target/release/orderflow-bubbles /usr/local/bin/
COPY --from=builder /app/dist /app/dist
WORKDIR /app
CMD ["orderflow-bubbles"]
```

Build and run:
```bash
docker build -t flow .
docker run -p 8080:8080 -e DATABENTO_API_KEY=$DATABENTO_API_KEY flow
```

### Systemd Service

Create `/etc/systemd/system/flow.service`:
```ini
[Unit]
Description=FLOW Orderflow Bubbles
After=network.target

[Service]
Type=simple
User=your-user
WorkingDirectory=/path/to/orderflow-bubbles
Environment="DATABENTO_API_KEY=your-key"
ExecStart=/path/to/orderflow-bubbles/target/release/orderflow-bubbles
Restart=always

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable flow
sudo systemctl start flow
```

---

## License

See ROADMAP.md for project details and strategy overview.
