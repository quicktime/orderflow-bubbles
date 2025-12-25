// WebSocket client for connecting to Rust backend

export interface Bubble {
  id: string;
  symbol: string;
  price: number;
  size: number;
  side: 'buy' | 'sell';
  timestamp: number;
  x: number;
  opacity: number;
  isSignificantImbalance: boolean;
}

export interface CVDPoint {
  timestamp: number;
  value: number;
  x: number;
}

export interface VolumeProfileLevel {
  price: number;
  buyVolume: number;
  sellVolume: number;
  totalVolume: number;
}

export interface AbsorptionZone {
  price: number;
  absorptionType: 'buying' | 'selling';
  totalAbsorbed: number;
  eventCount: number;
  firstSeen: number;
  lastSeen: number;
  strength: 'weak' | 'medium' | 'strong' | 'defended';
  atPoc: boolean;
  atVah: boolean;
  atVal: boolean;
  againstTrend: boolean;
}

export interface AbsorptionEvent {
  timestamp: number;
  price: number;
  absorptionType: 'buying' | 'selling';
  delta: number;
  priceChange: number;
  strength: 'weak' | 'medium' | 'strong' | 'defended';
  eventCount: number;
  totalAbsorbed: number;
  atKeyLevel: boolean;
  againstTrend: boolean;
  x: number;
}

export interface DeltaFlip {
  timestamp: number;
  flipType: 'zero_cross' | 'reversal';
  direction: 'bullish' | 'bearish';
  cvdBefore: number;
  cvdAfter: number;
  x: number;
}

export interface StackedImbalance {
  timestamp: number;
  side: 'buy' | 'sell';
  levelCount: number;
  priceHigh: number;
  priceLow: number;
  totalImbalance: number;
  x: number;
}

export interface ConfluenceEvent {
  timestamp: number;
  price: number;
  direction: 'bullish' | 'bearish';
  score: number; // 2 = medium, 3 = high, 4+ = very high
  signals: string[]; // List of contributing signals
  priceAfter1m: number | null;
  priceAfter5m: number | null;
  x: number;
}

export interface SignalStats {
  count: number;
  bullishCount: number;
  bearishCount: number;
  wins: number;
  losses: number;
  avgMove1m: number;
  avgMove5m: number;
  winRate: number;
}

export interface SessionStats {
  sessionStart: number;
  deltaFlips: SignalStats;
  absorptions: SignalStats;
  stackedImbalances: SignalStats;
  confluences: SignalStats;
  currentPrice: number;
  sessionHigh: number;
  sessionLow: number;
  totalVolume: number;
}

export interface ReplayStatus {
  mode: string;
  isPaused: boolean;
  speed: number;
  replayDate: string | null;
  replayProgress: number | null;
  currentTime: number | null;
}

export type WsMessage =
  | { type: 'Bubble' } & Bubble
  | { type: 'CVDPoint'; timestamp: number; value: number; x: number }
  | { type: 'VolumeProfile'; levels: VolumeProfileLevel[] }
  | { type: 'Absorption' } & AbsorptionEvent
  | { type: 'AbsorptionZones'; zones: AbsorptionZone[] }
  | { type: 'DeltaFlip' } & DeltaFlip
  | { type: 'StackedImbalance' } & StackedImbalance
  | { type: 'Confluence' } & ConfluenceEvent
  | { type: 'SessionStats' } & SessionStats
  | { type: 'ReplayStatus' } & ReplayStatus
  | { type: 'Connected'; symbols: string[]; mode: string }
  | { type: 'Error'; message: string };

export class RustWebSocket {
  private ws: WebSocket | null = null;
  private url: string;
  private reconnectAttempts = 0;
  private baseDelay = 1000;
  private maxDelay = 30000;
  private shouldReconnect = true;
  private onMessageCallback: ((message: WsMessage) => void) | null = null;
  private onConnectCallback: (() => void) | null = null;
  private onDisconnectCallback: (() => void) | null = null;
  private onReconnectingCallback: ((attempt: number, delay: number) => void) | null = null;

  constructor(url?: string) {
    if (url) {
      this.url = url;
    } else {
      // Auto-detect WebSocket URL based on current location
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const host = window.location.host;
      this.url = `${protocol}//${host}/ws`;
    }
  }

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.url);

        this.ws.onopen = () => {
          console.log('Connected to Rust backend');
          this.reconnectAttempts = 0;
          if (this.onConnectCallback) {
            this.onConnectCallback();
          }
          resolve();
        };

        this.ws.onmessage = (event) => {
          try {
            const message: WsMessage = JSON.parse(event.data);
            if (this.onMessageCallback) {
              this.onMessageCallback(message);
            }
          } catch (e) {
            console.error('Failed to parse message:', e);
          }
        };

        this.ws.onerror = (error) => {
          console.error('WebSocket error:', error);
          reject(error);
        };

        this.ws.onclose = () => {
          console.log('Disconnected from Rust backend');
          if (this.onDisconnectCallback) {
            this.onDisconnectCallback();
          }
          if (this.shouldReconnect) {
            this.attemptReconnect();
          }
        };
      } catch (e) {
        reject(e);
      }
    });
  }

  private attemptReconnect() {
    this.reconnectAttempts++;
    // Exponential backoff with jitter: delay = min(maxDelay, baseDelay * 2^attempts + random jitter)
    const exponentialDelay = this.baseDelay * Math.pow(2, Math.min(this.reconnectAttempts - 1, 5));
    const jitter = Math.random() * 1000;
    const delay = Math.min(this.maxDelay, exponentialDelay + jitter);

    console.log(`Reconnecting in ${(delay / 1000).toFixed(1)}s... (attempt ${this.reconnectAttempts})`);

    if (this.onReconnectingCallback) {
      this.onReconnectingCallback(this.reconnectAttempts, delay);
    }

    setTimeout(() => {
      if (this.shouldReconnect) {
        this.connect().catch((e) => console.error('Reconnect failed:', e));
      }
    }, delay);
  }

  disconnect() {
    this.shouldReconnect = false;
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  onMessage(callback: (message: WsMessage) => void) {
    this.onMessageCallback = callback;
  }

  onConnect(callback: () => void) {
    this.onConnectCallback = callback;
  }

  onDisconnect(callback: () => void) {
    this.onDisconnectCallback = callback;
  }

  onReconnecting(callback: (attempt: number, delay: number) => void) {
    this.onReconnectingCallback = callback;
  }

  getReconnectAttempts(): number {
    return this.reconnectAttempts;
  }

  send(message: any) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }

  // Replay control methods
  replayPause() {
    this.send({ action: 'replay_pause' });
  }

  replayResume() {
    this.send({ action: 'replay_resume' });
  }

  setReplaySpeed(speed: number) {
    this.send({ action: 'set_replay_speed', speed });
  }

  setMinSize(minSize: number) {
    this.send({ action: 'set_min_size', min_size: minSize });
  }
}
