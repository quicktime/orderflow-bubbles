// WebSocket client for connecting to Rust backend

export interface Bubble {
  id: string;
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

export type WsMessage =
  | { type: 'Bubble' } & Bubble
  | { type: 'CVDPoint'; timestamp: number; value: number; x: number }
  | { type: 'VolumeProfile'; levels: VolumeProfileLevel[] }
  | { type: 'Absorption' } & AbsorptionEvent
  | { type: 'AbsorptionZones'; zones: AbsorptionZone[] }
  | { type: 'Connected'; symbols: string[] }
  | { type: 'Error'; message: string };

export class RustWebSocket {
  private ws: WebSocket | null = null;
  private url: string;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;
  private onMessageCallback: ((message: WsMessage) => void) | null = null;
  private onConnectCallback: (() => void) | null = null;
  private onDisconnectCallback: (() => void) | null = null;

  constructor(url: string = 'ws://localhost:8080/ws') {
    this.url = url;
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
          this.attemptReconnect();
        };
      } catch (e) {
        reject(e);
      }
    });
  }

  private attemptReconnect() {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      console.log(
        `Reconnecting... (${this.reconnectAttempts}/${this.maxReconnectAttempts})`
      );
      setTimeout(() => {
        this.connect().catch((e) => console.error('Reconnect failed:', e));
      }, this.reconnectDelay * this.reconnectAttempts);
    }
  }

  disconnect() {
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

  send(message: any) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(message));
    }
  }

  isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }
}
