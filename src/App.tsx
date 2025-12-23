import { useEffect, useRef, useState, useCallback } from 'react';
import { RustWebSocket, WsMessage } from './websocket';
import { BubbleRenderer } from './BubbleRenderer';
import './App.css';

interface Bubble {
  id: string;
  price: number;
  size: number;
  side: 'buy' | 'sell';
  timestamp: number;
  x: number;
  opacity: number;
  isSignificantImbalance?: boolean;
}

interface CVDPoint {
  timestamp: number;
  value: number;
  x: number;
}

interface ZeroCross {
  timestamp: number;
  direction: 'bullish' | 'bearish';
  x: number;
  price?: number;
}

interface AbsorptionAlert {
  timestamp: number;
  price: number;
  absorptionType: 'buying' | 'selling';
  delta: number;
  strength: 'weak' | 'medium' | 'strong' | 'defended';
  eventCount: number;
  totalAbsorbed: number;
  atKeyLevel: boolean;
  againstTrend: boolean;
  x: number;
}

interface AbsorptionZone {
  price: number;
  absorptionType: 'buying' | 'selling';
  totalAbsorbed: number;
  eventCount: number;
  strength: 'weak' | 'medium' | 'strong' | 'defended';
  atPoc: boolean;
  atVah: boolean;
  atVal: boolean;
  againstTrend: boolean;
}

interface VolumeProfileLevel {
  price: number;
  buyVolume: number;
  sellVolume: number;
  totalVolume: number;
}

interface StackedImbalance {
  timestamp: number;
  side: 'buy' | 'sell';
  levelCount: number;
  priceHigh: number;
  priceLow: number;
  totalImbalance: number;
  x: number;
}

interface ConfluenceEvent {
  timestamp: number;
  price: number;
  direction: 'bullish' | 'bearish';
  score: number;
  signals: string[];
  x: number;
}

interface SignalStats {
  count: number;
  bullishCount: number;
  bearishCount: number;
  wins: number;
  losses: number;
  avgMove1m: number;
  avgMove5m: number;
  winRate: number;
}

interface SessionStats {
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

const BUBBLE_LIFETIME_SECONDS = 120;

// Audio alert function for zero crosses
function playAlertSound(direction: 'bullish' | 'bearish') {
  try {
    const audioContext = new (window.AudioContext || (window as any).webkitAudioContext)();
    const oscillator = audioContext.createOscillator();
    const gainNode = audioContext.createGain();

    oscillator.connect(gainNode);
    gainNode.connect(audioContext.destination);

    oscillator.frequency.value = direction === 'bullish' ? 800 : 400;
    oscillator.type = 'sine';

    gainNode.gain.setValueAtTime(0.3, audioContext.currentTime);
    gainNode.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.3);

    oscillator.start(audioContext.currentTime);
    oscillator.stop(audioContext.currentTime + 0.3);
  } catch (e) {
    console.log('Audio not supported', e);
  }
}

// Audio alert for stacked imbalances - triple ascending/descending beep
function playStackedImbalanceSound(side: 'buy' | 'sell') {
  try {
    const audioContext = new (window.AudioContext || (window as any).webkitAudioContext)();
    const baseFreq = side === 'buy' ? 500 : 400;
    const freqStep = side === 'buy' ? 100 : -50;

    for (let i = 0; i < 3; i++) {
      const osc = audioContext.createOscillator();
      const gain = audioContext.createGain();
      osc.connect(gain);
      gain.connect(audioContext.destination);
      osc.frequency.value = baseFreq + (freqStep * i);
      osc.type = 'square';
      const startTime = audioContext.currentTime + (i * 0.1);
      gain.gain.setValueAtTime(0.15, startTime);
      gain.gain.exponentialRampToValueAtTime(0.01, startTime + 0.08);
      osc.start(startTime);
      osc.stop(startTime + 0.08);
    }
  } catch (e) {
    console.log('Audio not supported', e);
  }
}

// Audio alert for absorption events - double beep
function playAbsorptionSound(type: 'buying' | 'selling') {
  try {
    const audioContext = new (window.AudioContext || (window as any).webkitAudioContext)();

    // First beep
    const osc1 = audioContext.createOscillator();
    const gain1 = audioContext.createGain();
    osc1.connect(gain1);
    gain1.connect(audioContext.destination);
    osc1.frequency.value = type === 'buying' ? 600 : 300;
    osc1.type = 'triangle';
    gain1.gain.setValueAtTime(0.2, audioContext.currentTime);
    gain1.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.1);
    osc1.start(audioContext.currentTime);
    osc1.stop(audioContext.currentTime + 0.1);

    // Second beep (slightly delayed)
    const osc2 = audioContext.createOscillator();
    const gain2 = audioContext.createGain();
    osc2.connect(gain2);
    gain2.connect(audioContext.destination);
    osc2.frequency.value = type === 'buying' ? 700 : 350;
    osc2.type = 'triangle';
    gain2.gain.setValueAtTime(0.2, audioContext.currentTime + 0.15);
    gain2.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + 0.25);
    osc2.start(audioContext.currentTime + 0.15);
    osc2.stop(audioContext.currentTime + 0.25);
  } catch (e) {
    console.log('Audio not supported', e);
  }
}

// Audio alert for confluence - distinctive chord
function playConfluenceSound(direction: 'bullish' | 'bearish') {
  try {
    const audioContext = new (window.AudioContext || (window as any).webkitAudioContext)();
    const baseFreq = direction === 'bullish' ? 400 : 300;
    const freqs = [baseFreq, baseFreq * 1.25, baseFreq * 1.5]; // Major chord

    freqs.forEach((freq, i) => {
      const osc = audioContext.createOscillator();
      const gain = audioContext.createGain();
      osc.connect(gain);
      gain.connect(audioContext.destination);
      osc.frequency.value = freq;
      osc.type = 'sine';
      const delay = i * 0.05;
      gain.gain.setValueAtTime(0.15, audioContext.currentTime + delay);
      gain.gain.exponentialRampToValueAtTime(0.01, audioContext.currentTime + delay + 0.4);
      osc.start(audioContext.currentTime + delay);
      osc.stop(audioContext.currentTime + delay + 0.4);
    });
  } catch (e) {
    console.log('Audio not supported', e);
  }
}

function App() {
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [lastPrice, setLastPrice] = useState<number | null>(null);
  const [priceRange, setPriceRange] = useState<{ min: number; max: number } | null>(null);
  const [cvdHistory, setCvdHistory] = useState<CVDPoint[]>([]);
  const [currentCVD, setCurrentCVD] = useState(0);
  const [cvdRange, setCvdRange] = useState<{ min: number; max: number }>({ min: 0, max: 0 });
  const [zeroCrosses, setZeroCrosses] = useState<ZeroCross[]>([]);
  const [cvdFlashAlert, setCvdFlashAlert] = useState<'bullish' | 'bearish' | null>(null);
  const [showCvdBadge, setShowCvdBadge] = useState<'bullish' | 'bearish' | null>(null);
  const [volumeProfile, setVolumeProfile] = useState<Map<number, VolumeProfileLevel>>(new Map());
  const [isPaused, setIsPaused] = useState(false);
  const [selectedBubble, setSelectedBubble] = useState<Bubble | null>(null);
  const [clickPosition, setClickPosition] = useState<{ x: number; y: number } | null>(null);
  const [showShortcutsHelp, setShowShortcutsHelp] = useState(false);
  const [isSoundEnabled, setIsSoundEnabled] = useState(true);
  const [cvdStartTime, setCvdStartTime] = useState<number>(Date.now());
  const [_absorptionAlerts, setAbsorptionAlerts] = useState<AbsorptionAlert[]>([]); // eslint-disable-line @typescript-eslint/no-unused-vars
  const [absorptionZones, setAbsorptionZones] = useState<AbsorptionZone[]>([]); // Passed to BubbleRenderer for canvas rendering
  const [showAbsorptionBadge, setShowAbsorptionBadge] = useState<AbsorptionAlert | null>(null);
  const [stackedImbalances, setStackedImbalances] = useState<StackedImbalance[]>([]);
  const [showStackedBadge, setShowStackedBadge] = useState<StackedImbalance | null>(null);
  const [_confluenceEvents, setConfluenceEvents] = useState<ConfluenceEvent[]>([]); // eslint-disable-line @typescript-eslint/no-unused-vars
  const [showConfluenceBadge, setShowConfluenceBadge] = useState<ConfluenceEvent | null>(null);
  const [sessionStats, setSessionStats] = useState<SessionStats | null>(null);
  const [currentView, setCurrentView] = useState<'chart' | 'stats'>('chart');

  const wsRef = useRef<RustWebSocket | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const cvdBaselineRef = useRef<number>(0); // Offset for CVD reset
  const lastRawCvdRef = useRef<number>(0);  // Track last raw CVD from server
  const prevAdjustedCvdRef = useRef<number>(0); // Track previous adjusted CVD for zero-cross

  // Connect to Rust backend
  useEffect(() => {
    const ws = new RustWebSocket();
    wsRef.current = ws;

    ws.onConnect(() => {
      setIsConnected(true);
      setError(null);
      console.log('✅ Connected to Rust backend');
    });

    ws.onDisconnect(() => {
      setIsConnected(false);
      console.log('❌ Disconnected from Rust backend');
    });

    ws.onMessage((message: WsMessage) => {
      switch (message.type) {
        case 'Bubble':
          const bubble: Bubble = {
            id: message.id,
            price: message.price,
            size: message.size,
            side: message.side,
            timestamp: message.timestamp,
            x: message.x,
            opacity: message.opacity,
            isSignificantImbalance: message.isSignificantImbalance,
          };

          setBubbles((prev) => [...prev, bubble]);

          // Update last price and price range
          setLastPrice(bubble.price);
          setPriceRange((prev) => {
            if (!prev) {
              return { min: bubble.price - 10, max: bubble.price + 10 };
            }
            const padding = (prev.max - prev.min) * 0.1;
            return {
              min: Math.min(prev.min, bubble.price - padding),
              max: Math.max(prev.max, bubble.price + padding),
            };
          });
          break;

        case 'CVDPoint':
          // Apply baseline offset for reset functionality
          const rawCvd = message.value;
          lastRawCvdRef.current = rawCvd;
          const adjustedCvd = rawCvd - cvdBaselineRef.current;

          const cvdPoint: CVDPoint = {
            timestamp: message.timestamp,
            value: adjustedCvd,
            x: message.x,
          };

          setCvdHistory((prev) => [...prev, cvdPoint]);
          setCurrentCVD(adjustedCvd);

          // Update CVD range
          setCvdRange((prev) => ({
            min: Math.min(prev.min, adjustedCvd),
            max: Math.max(prev.max, adjustedCvd),
          }));

          // Zero-cross detection using refs to avoid stale closure
          const prevCvd = prevAdjustedCvdRef.current;
          prevAdjustedCvdRef.current = adjustedCvd;

          const prevSign = Math.sign(prevCvd);
          const newSign = Math.sign(adjustedCvd);

          if (prevSign !== 0 && newSign !== 0 && prevSign !== newSign && Math.abs(prevCvd) >= 300) {
            const direction = adjustedCvd > 0 ? 'bullish' : 'bearish';
            console.log(`🚨 CVD ZERO CROSS: ${direction.toUpperCase()}`);

            setZeroCrosses((prev) => [
              ...prev,
              {
                timestamp: Date.now(),
                direction,
                x: 0.92,
                price: lastPrice || undefined,
              },
            ]);

            setCvdFlashAlert(direction);
            setTimeout(() => setCvdFlashAlert(null), 500);

            setShowCvdBadge(direction);
            setTimeout(() => setShowCvdBadge(null), 3000);

            if (isSoundEnabled) {
              playAlertSound(direction);
            }
          }
          break;

        case 'VolumeProfile':
          const profile = new Map<number, VolumeProfileLevel>();
          message.levels.forEach((level) => {
            profile.set(level.price, level);
          });
          setVolumeProfile(profile);
          break;

        case 'Absorption':
          const absorption: AbsorptionAlert = {
            timestamp: message.timestamp,
            price: message.price,
            absorptionType: message.absorptionType,
            delta: message.delta,
            strength: message.strength,
            eventCount: message.eventCount,
            totalAbsorbed: message.totalAbsorbed,
            atKeyLevel: message.atKeyLevel,
            againstTrend: message.againstTrend,
            x: message.x,
          };

          console.log(
            `🛡️ ABSORPTION [${absorption.strength.toUpperCase()}]: ${absorption.absorptionType} absorbed at ${absorption.price.toFixed(2)} | events=${absorption.eventCount} total=${absorption.totalAbsorbed} ${absorption.atKeyLevel ? '@ KEY LEVEL' : ''} ${absorption.againstTrend ? '⚠️ AGAINST TREND' : ''}`
          );

          setAbsorptionAlerts((prev) => [...prev, absorption]);

          // Only show badge for medium+ strength
          if (absorption.strength !== 'weak') {
            setShowAbsorptionBadge(absorption);
            setTimeout(() => setShowAbsorptionBadge(null), 4000);

            if (isSoundEnabled) {
              playAbsorptionSound(absorption.absorptionType);
            }
          }
          break;

        case 'AbsorptionZones':
          setAbsorptionZones(message.zones.map(z => ({
            price: z.price,
            absorptionType: z.absorptionType,
            totalAbsorbed: z.totalAbsorbed,
            eventCount: z.eventCount,
            strength: z.strength,
            atPoc: z.atPoc,
            atVah: z.atVah,
            atVal: z.atVal,
            againstTrend: z.againstTrend,
          })));
          break;

        case 'DeltaFlip':
          console.log(`⚡ DELTA FLIP from backend: ${message.direction.toUpperCase()} (${message.flipType})`);

          // Add to zeroCrosses for visual display
          setZeroCrosses((prev) => [
            ...prev,
            {
              timestamp: message.timestamp,
              direction: message.direction,
              x: message.x,
              price: lastPrice || undefined,
            },
          ]);

          // Trigger visual alerts
          setCvdFlashAlert(message.direction);
          setTimeout(() => setCvdFlashAlert(null), 500);

          setShowCvdBadge(message.direction);
          setTimeout(() => setShowCvdBadge(null), 3000);

          if (isSoundEnabled) {
            playAlertSound(message.direction);
          }
          break;

        case 'StackedImbalance':
          console.log(
            `📊 STACKED IMBALANCE [${message.side.toUpperCase()}]: ${message.levelCount} levels from ${message.priceLow.toFixed(2)} to ${message.priceHigh.toFixed(2)}`
          );

          const stacked: StackedImbalance = {
            timestamp: message.timestamp,
            side: message.side,
            levelCount: message.levelCount,
            priceHigh: message.priceHigh,
            priceLow: message.priceLow,
            totalImbalance: message.totalImbalance,
            x: message.x,
          };

          setStackedImbalances((prev) => [...prev, stacked]);

          // Show badge for 4+ levels (strong signal)
          if (message.levelCount >= 4) {
            setShowStackedBadge(stacked);
            setTimeout(() => setShowStackedBadge(null), 3000);

            if (isSoundEnabled) {
              playStackedImbalanceSound(message.side);
            }
          }
          break;

        case 'Confluence':
          console.log(
            `🎯 CONFLUENCE [${message.score >= 3 ? 'HIGH' : 'MEDIUM'}]: ${message.signals.join(' + ')} → ${message.direction.toUpperCase()}`
          );

          const confluence: ConfluenceEvent = {
            timestamp: message.timestamp,
            price: message.price,
            direction: message.direction,
            score: message.score,
            signals: message.signals,
            x: message.x,
          };

          setConfluenceEvents((prev) => [...prev, confluence]);

          // Always show confluence badge - it's a high-value signal
          setShowConfluenceBadge(confluence);
          setTimeout(() => setShowConfluenceBadge(null), 5000);

          if (isSoundEnabled) {
            playConfluenceSound(message.direction);
          }
          break;

        case 'SessionStats':
          setSessionStats({
            sessionStart: message.sessionStart,
            deltaFlips: message.deltaFlips,
            absorptions: message.absorptions,
            stackedImbalances: message.stackedImbalances,
            confluences: message.confluences,
            currentPrice: message.currentPrice,
            sessionHigh: message.sessionHigh,
            sessionLow: message.sessionLow,
            totalVolume: message.totalVolume,
          });
          break;

        case 'Connected':
          console.log('📡 Connected to symbols:', message.symbols);
          break;

        case 'Error':
          console.error('Backend error:', message.message);
          setError(message.message);
          break;
      }
    });

    ws.connect().catch((e) => {
      console.error('Failed to connect:', e);
      setError('Failed to connect to Rust backend. Make sure the server is running.');
    });

    return () => {
      ws.disconnect();
    };
  }, []); // Only run once on mount

  // Reset CVD function
  const resetCVD = useCallback(() => {
    // Set baseline to current raw CVD so future values start from 0
    cvdBaselineRef.current = lastRawCvdRef.current;
    prevAdjustedCvdRef.current = 0;
    setCurrentCVD(0);
    setCvdHistory([]);
    setCvdRange({ min: 0, max: 0 });
    setZeroCrosses([]);
    setCvdStartTime(Date.now());
    console.log('🔄 CVD RESET - Starting fresh (baseline set to', lastRawCvdRef.current, ')');
  }, []);

  // Export screenshot
  const exportScreenshot = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    try {
      canvas.toBlob((blob) => {
        if (!blob) return;

        const url = URL.createObjectURL(blob);
        const link = document.createElement('a');
        const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, -5);
        link.download = `flow-orderflow-${timestamp}.png`;
        link.href = url;
        link.click();

        URL.revokeObjectURL(url);
        console.log('📸 Screenshot exported');
      }, 'image/png');
    } catch (err) {
      console.error('Failed to export screenshot:', err);
    }
  }, []);

  // Handle canvas click to show bubble info
  const handleCanvasClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current;
      if (!canvas || !priceRange) return;

      const rect = canvas.getBoundingClientRect();
      const clickX = e.clientX - rect.left;
      const clickY = e.clientY - rect.top;

      const normalizedX = clickX / rect.width;
      const normalizedY = clickY / rect.height;

      const priceSpan = priceRange.max - priceRange.min;

      let clickedBubble: Bubble | null = null;
      let minDistance = Infinity;

      for (let i = bubbles.length - 1; i >= 0; i--) {
        const bubble = bubbles[i];
        const bubbleX = bubble.x;
        const bubbleY = 1 - (bubble.price - priceRange.min) / priceSpan;
        const radius = Math.min(100, Math.max(3, bubble.size * 0.008)) / rect.width;

        const dx = normalizedX - bubbleX;
        const dy = normalizedY - bubbleY;
        const distance = Math.sqrt(dx * dx + dy * dy);

        if (distance <= radius && distance < minDistance) {
          clickedBubble = bubble;
          minDistance = distance;
        }
      }

      if (clickedBubble) {
        setSelectedBubble(clickedBubble);
        setClickPosition({ x: clickX, y: clickY });
      } else {
        setSelectedBubble(null);
        setClickPosition(null);
      }
    },
    [bubbles, priceRange]
  );

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === 'INPUT') return;

      const key = e.key.toLowerCase();

      if (key === 'escape') {
        if (showShortcutsHelp) {
          setShowShortcutsHelp(false);
          return;
        }
        if (selectedBubble) {
          setSelectedBubble(null);
          setClickPosition(null);
          return;
        }
      }

      if (key === '?' || (e.shiftKey && key === '/')) {
        setShowShortcutsHelp((prev) => !prev);
        return;
      }

      if (showShortcutsHelp) return;

      switch (key) {
        case 'r':
          resetCVD();
          console.log('⌨️ Keyboard: CVD Reset (R)');
          break;
        case ' ':
          e.preventDefault();
          setIsPaused((prev) => {
            console.log(`⌨️ Keyboard: ${!prev ? 'Paused' : 'Resumed'} (Space)`);
            return !prev;
          });
          break;
        case 'c':
          setBubbles([]);
          console.log('⌨️ Keyboard: Cleared bubbles (C)');
          break;
        case 'm':
          setIsSoundEnabled((prev) => {
            console.log(`⌨️ Keyboard: Sound ${!prev ? 'Enabled' : 'Muted'} (M)`);
            return !prev;
          });
          break;
        case 's':
          exportScreenshot();
          break;
      }
    };

    window.addEventListener('keydown', handleKeyPress);
    return () => window.removeEventListener('keydown', handleKeyPress);
  }, [resetCVD, exportScreenshot, showShortcutsHelp, selectedBubble]);

  // Animation loop - TIME-BASED
  useEffect(() => {
    let animationFrameId: number;
    let lastFrameTime = performance.now();
    let lastCleanupTime = performance.now();

    const SPEED_PER_SECOND = (0.77 / BUBBLE_LIFETIME_SECONDS) * 3; // 3x faster panning
    const CLEANUP_INTERVAL_MS = 1000;

    const animate = (currentTime: number) => {
      if (isPaused) {
        lastFrameTime = currentTime;
        lastCleanupTime = currentTime;
        animationFrameId = requestAnimationFrame(animate);
        return;
      }

      const deltaTime = (currentTime - lastFrameTime) / 1000;
      lastFrameTime = currentTime;

      const movement = SPEED_PER_SECOND * deltaTime;

      const shouldCleanup = currentTime - lastCleanupTime >= CLEANUP_INTERVAL_MS;
      if (shouldCleanup) {
        lastCleanupTime = currentTime;

        const now = Date.now();
        const maxAge = BUBBLE_LIFETIME_SECONDS * 1000;

        setBubbles((prev) => prev.filter((b) => now - b.timestamp < maxAge));
        setCvdHistory((prev) => prev.filter((p) => now - p.timestamp < maxAge));
        setZeroCrosses((prev) => prev.filter((c) => now - c.timestamp < maxAge));
        setAbsorptionAlerts((prev) => prev.filter((a) => now - a.timestamp < maxAge));
        setStackedImbalances((prev) => prev.filter((s) => now - s.timestamp < maxAge));
        setConfluenceEvents((prev) => prev.filter((c) => now - c.timestamp < maxAge));
      }

      setBubbles((prev) =>
        prev.map((bubble) => ({
          ...bubble,
          x: bubble.x - movement,
          opacity: 1,
        }))
      );

      setCvdHistory((prev) =>
        prev.map((point) => ({
          ...point,
          x: point.x - movement,
        }))
      );

      setZeroCrosses((prev) =>
        prev.map((cross) => ({
          ...cross,
          x: cross.x - movement,
        }))
      );

      setAbsorptionAlerts((prev) =>
        prev.map((alert) => ({
          ...alert,
          x: alert.x - movement,
        }))
      );

      setStackedImbalances((prev) =>
        prev.map((stacked) => ({
          ...stacked,
          x: stacked.x - movement,
        }))
      );

      setConfluenceEvents((prev) =>
        prev.map((conf) => ({
          ...conf,
          x: conf.x - movement,
        }))
      );

      animationFrameId = requestAnimationFrame(animate);
    };

    animationFrameId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animationFrameId);
  }, [isPaused]);

  return (
    <div className="app">
      <header className="header">
        <div className="header-left">
          <h1 className="logo">
            <span className="logo-icon">◉</span>
            FLOW
          </h1>
        </div>

        <div className="header-center">
          {lastPrice && (
            <div className="last-price">
              <span className="price-label">LAST</span>
              <span className="price-value">{lastPrice.toFixed(2)}</span>
            </div>
          )}
        </div>

        <div className="header-right">
          {isConnected && (
            <>
              <div className={`cvd-widget ${currentCVD >= 0 ? 'bullish' : 'bearish'}`}>
                <label>CVD</label>
                <div className="cvd-value">
                  {currentCVD > 0 ? '+' : ''}
                  {currentCVD.toFixed(0)}
                </div>
                <div className="cvd-direction">
                  {currentCVD >= 0 ? '↗ BULLISH' : '↘ BEARISH'}
                </div>
                <div className="cvd-age">
                  Since{' '}
                  {new Date(cvdStartTime).toLocaleTimeString('en-US', {
                    hour: 'numeric',
                    minute: '2-digit',
                    hour12: true,
                  })}
                </div>
              </div>
              <button className="reset-cvd-btn" onClick={resetCVD} title="Reset CVD to zero">
                🔄
              </button>
              <button
                className={`sound-toggle-btn ${isSoundEnabled ? 'enabled' : 'disabled'}`}
                onClick={() => setIsSoundEnabled(!isSoundEnabled)}
                title={isSoundEnabled ? 'Mute alerts' : 'Unmute alerts'}
              >
                {isSoundEnabled ? '🔊' : '🔇'}
              </button>
            </>
          )}

          <div className={`status ${isConnected ? 'connected' : ''}`}>
            <span className="status-dot"></span>
            {isConnected ? 'LIVE' : 'OFFLINE'}
          </div>
          {isPaused && (
            <div className="paused-indicator" title="Press Space to resume">
              ⏸ PAUSED
            </div>
          )}
          <button
            className="shortcuts-help-btn"
            onClick={() => setShowShortcutsHelp(!showShortcutsHelp)}
            title="Keyboard shortcuts"
          >
            ⌨️
          </button>
          {isConnected && (
            <button className="screenshot-btn" onClick={exportScreenshot} title="Export screenshot (S)">
              📸
            </button>
          )}
          {isConnected && (
            <button
              className={`view-toggle-btn ${currentView === 'stats' ? 'active' : ''}`}
              onClick={() => setCurrentView(currentView === 'chart' ? 'stats' : 'chart')}
              title={currentView === 'chart' ? 'View Session Stats' : 'Back to Chart'}
            >
              {currentView === 'chart' ? '📊' : '📈'}
            </button>
          )}
        </div>
      </header>

      {/* Keyboard Shortcuts Help Modal */}
      {showShortcutsHelp && (
        <div className="shortcuts-modal-overlay" onClick={() => setShowShortcutsHelp(false)}>
          <div className="shortcuts-modal" onClick={(e) => e.stopPropagation()}>
            <div className="shortcuts-modal-header">
              <h3>⌨️ Keyboard Shortcuts</h3>
              <button className="close-modal-btn" onClick={() => setShowShortcutsHelp(false)}>
                ✕
              </button>
            </div>
            <div className="shortcuts-grid">
              <div className="shortcut-section">
                <h4>General Controls</h4>
                <div className="shortcut-item">
                  <kbd>Space</kbd>
                  <span>Pause/Resume animation</span>
                </div>
                <div className="shortcut-item">
                  <kbd>R</kbd>
                  <span>Reset CVD to zero</span>
                </div>
                <div className="shortcut-item">
                  <kbd>C</kbd>
                  <span>Clear all bubbles</span>
                </div>
                <div className="shortcut-item">
                  <kbd>M</kbd>
                  <span>Mute/Unmute alerts</span>
                </div>
                <div className="shortcut-item">
                  <kbd>S</kbd>
                  <span>Export screenshot</span>
                </div>
              </div>
              <div className="shortcut-section">
                <h4>Interactions</h4>
                <div className="shortcut-item">
                  <kbd>Click</kbd>
                  <span>Show bubble details</span>
                </div>
                <div className="shortcut-item">
                  <kbd>Esc</kbd>
                  <span>Close this help</span>
                </div>
              </div>
            </div>
            <div className="shortcuts-modal-footer">
              Press <kbd>?</kbd> or click <span style={{ fontSize: '16px' }}>⌨️</span> to toggle
              this help
            </div>
          </div>
        </div>
      )}

      {error && (
        <div className="error-banner">
          ⚠️ {error}
          <button onClick={() => setError(null)}>✕</button>
        </div>
      )}

      <div className="visualization">
        {/* Flash Alert Overlay */}
        {cvdFlashAlert && <div className={`flash-alert ${cvdFlashAlert}`}></div>}

        {/* CVD Flip Badge */}
        {showCvdBadge && (
          <div className={`cvd-badge ${showCvdBadge}`}>
            <div className="badge-icon">{showCvdBadge === 'bullish' ? '🟢' : '🔴'}</div>
            <div className="badge-text">CVD FLIP: {showCvdBadge.toUpperCase()}</div>
            <div className="badge-subtitle">
              {showCvdBadge === 'bullish' ? 'Buy Signal' : 'Sell Signal'}
            </div>
          </div>
        )}

        {/* Absorption Badge */}
        {showAbsorptionBadge && (
          <div className={`absorption-badge ${showAbsorptionBadge.absorptionType} ${showAbsorptionBadge.strength}`}>
            <div className={`strength-indicator ${showAbsorptionBadge.strength}`}>
              {showAbsorptionBadge.strength.toUpperCase()}
            </div>
            <div className="badge-icon">
              {showAbsorptionBadge.strength === 'defended' ? '🔥' : '🛡️'}
            </div>
            <div className="badge-text">
              {showAbsorptionBadge.strength === 'defended' ? 'DEFENDED LEVEL' : 'ABSORPTION'}
            </div>
            <div className="badge-type">
              {showAbsorptionBadge.absorptionType === 'buying'
                ? 'Sellers absorbing buyers'
                : 'Buyers absorbing sellers'}
            </div>
            <div className="badge-stats">
              <span className="stat">
                <span className="stat-label">Events</span>
                <span className="stat-value">{showAbsorptionBadge.eventCount}x</span>
              </span>
              <span className="stat">
                <span className="stat-label">Volume</span>
                <span className="stat-value">{showAbsorptionBadge.totalAbsorbed}</span>
              </span>
              <span className="stat">
                <span className="stat-label">Price</span>
                <span className="stat-value">{showAbsorptionBadge.price.toFixed(2)}</span>
              </span>
            </div>
            {(showAbsorptionBadge.atKeyLevel || showAbsorptionBadge.againstTrend) && (
              <div className="badge-context">
                {showAbsorptionBadge.atKeyLevel && <span className="context-tag key-level">@ KEY LEVEL</span>}
                {showAbsorptionBadge.againstTrend && <span className="context-tag against-trend">⚠️ AGAINST TREND</span>}
              </div>
            )}
            <div className="badge-subtitle">
              {showAbsorptionBadge.strength === 'defended'
                ? 'High probability reversal zone'
                : showAbsorptionBadge.strength === 'strong'
                ? 'Strong institutional defense'
                : 'Building absorption zone'}
            </div>
          </div>
        )}

        {/* Stacked Imbalance Badge */}
        {showStackedBadge && (
          <div className={`stacked-badge ${showStackedBadge.side}`}>
            <div className="badge-icon">
              {showStackedBadge.side === 'buy' ? '📈' : '📉'}
            </div>
            <div className="badge-text">STACKED IMBALANCE</div>
            <div className="badge-type">
              {showStackedBadge.levelCount} consecutive {showStackedBadge.side} levels
            </div>
            <div className="badge-stats">
              <span className="stat">
                <span className="stat-label">Levels</span>
                <span className="stat-value">{showStackedBadge.levelCount}</span>
              </span>
              <span className="stat">
                <span className="stat-label">Range</span>
                <span className="stat-value">{showStackedBadge.priceLow.toFixed(2)} - {showStackedBadge.priceHigh.toFixed(2)}</span>
              </span>
            </div>
            <div className="badge-subtitle">
              Strong {showStackedBadge.side === 'buy' ? 'buying' : 'selling'} pressure - expect continuation
            </div>
          </div>
        )}

        {/* Confluence Badge */}
        {showConfluenceBadge && (
          <div className={`confluence-badge ${showConfluenceBadge.direction}`}>
            <div className="badge-icon">🎯</div>
            <div className="badge-text">CONFLUENCE</div>
            <div className="badge-type">
              {showConfluenceBadge.score >= 3 ? 'HIGH PROBABILITY' : 'MEDIUM PROBABILITY'} {showConfluenceBadge.direction.toUpperCase()}
            </div>
            <div className="badge-signals">
              {showConfluenceBadge.signals.map((signal, i) => (
                <span key={i} className="signal-tag">{signal.replace('_', ' ')}</span>
              ))}
            </div>
            <div className="badge-stats">
              <span className="stat">
                <span className="stat-label">Score</span>
                <span className="stat-value">{showConfluenceBadge.score}/4</span>
              </span>
              <span className="stat">
                <span className="stat-label">Price</span>
                <span className="stat-value">{showConfluenceBadge.price.toFixed(2)}</span>
              </span>
            </div>
            <div className="badge-subtitle">
              {showConfluenceBadge.direction === 'bullish'
                ? 'Multiple signals agree - consider LONG entry'
                : 'Multiple signals agree - consider SHORT entry'}
            </div>
          </div>
        )}

        {currentView === 'chart' ? (
          <>
            <BubbleRenderer
          bubbles={bubbles}
          priceRange={priceRange}
          canvasRef={canvasRef}
          cvdHistory={cvdHistory}
          cvdRange={cvdRange}
          currentCVD={currentCVD}
          zeroCrosses={zeroCrosses}
          onClick={handleCanvasClick}
          volumeProfile={volumeProfile}
          absorptionZones={absorptionZones}
          stackedImbalances={stackedImbalances}
        />

        {/* Bubble Info Tooltip */}
        {selectedBubble && clickPosition && (
          <div
            className="bubble-info-tooltip"
            style={{
              left: `${clickPosition.x}px`,
              top: `${clickPosition.y}px`,
            }}
            onClick={() => {
              setSelectedBubble(null);
              setClickPosition(null);
            }}
          >
            <div className="tooltip-header">
              <span className={`tooltip-side ${selectedBubble.side}`}>
                {selectedBubble.side.toUpperCase()}
              </span>
            </div>
            <div className="tooltip-row">
              <span className="tooltip-label">Size:</span>
              <span className="tooltip-value">{selectedBubble.size} contracts</span>
            </div>
            <div className="tooltip-row">
              <span className="tooltip-label">Price:</span>
              <span className="tooltip-value">{selectedBubble.price.toFixed(2)}</span>
            </div>
            <div className="tooltip-row">
              <span className="tooltip-label">Time:</span>
              <span className="tooltip-value">
                {new Date(selectedBubble.timestamp).toLocaleTimeString()}
              </span>
            </div>
            <div className="tooltip-footer">Click to close</div>
          </div>
        )}
          </>
        ) : (
          /* Stats View */
          <div className="stats-view">
            <h2>Session Statistics</h2>
            {sessionStats ? (
              <div className="stats-grid">
                <div className="stats-overview">
                  <div className="overview-card">
                    <span className="overview-label">Session Start</span>
                    <span className="overview-value">
                      {new Date(sessionStats.sessionStart).toLocaleTimeString()}
                    </span>
                  </div>
                  <div className="overview-card">
                    <span className="overview-label">Current Price</span>
                    <span className="overview-value">{sessionStats.currentPrice.toFixed(2)}</span>
                  </div>
                  <div className="overview-card">
                    <span className="overview-label">Session Range</span>
                    <span className="overview-value">
                      {sessionStats.sessionLow.toFixed(2)} - {sessionStats.sessionHigh.toFixed(2)}
                    </span>
                  </div>
                  <div className="overview-card">
                    <span className="overview-label">Total Volume</span>
                    <span className="overview-value">{sessionStats.totalVolume.toLocaleString()}</span>
                  </div>
                </div>

                <div className="signal-stats-grid">
                  {/* Delta Flips */}
                  <div className="signal-card">
                    <h3>Delta Flips</h3>
                    <div className="signal-counts">
                      <span className="count total">{sessionStats.deltaFlips.count} total</span>
                      <span className="count bullish">{sessionStats.deltaFlips.bullishCount} bullish</span>
                      <span className="count bearish">{sessionStats.deltaFlips.bearishCount} bearish</span>
                    </div>
                    <div className="signal-metrics">
                      <div className="metric">
                        <span className="metric-label">Win Rate</span>
                        <span className={`metric-value ${sessionStats.deltaFlips.winRate >= 50 ? 'positive' : 'negative'}`}>
                          {sessionStats.deltaFlips.winRate.toFixed(1)}%
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (1m)</span>
                        <span className={`metric-value ${sessionStats.deltaFlips.avgMove1m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.deltaFlips.avgMove1m >= 0 ? '+' : ''}{sessionStats.deltaFlips.avgMove1m.toFixed(2)}
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (5m)</span>
                        <span className={`metric-value ${sessionStats.deltaFlips.avgMove5m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.deltaFlips.avgMove5m >= 0 ? '+' : ''}{sessionStats.deltaFlips.avgMove5m.toFixed(2)}
                        </span>
                      </div>
                    </div>
                    <div className="win-loss">
                      <span className="wins">{sessionStats.deltaFlips.wins} W</span>
                      <span className="losses">{sessionStats.deltaFlips.losses} L</span>
                    </div>
                  </div>

                  {/* Absorptions */}
                  <div className="signal-card">
                    <h3>Absorptions</h3>
                    <div className="signal-counts">
                      <span className="count total">{sessionStats.absorptions.count} total</span>
                      <span className="count bullish">{sessionStats.absorptions.bullishCount} bullish</span>
                      <span className="count bearish">{sessionStats.absorptions.bearishCount} bearish</span>
                    </div>
                    <div className="signal-metrics">
                      <div className="metric">
                        <span className="metric-label">Win Rate</span>
                        <span className={`metric-value ${sessionStats.absorptions.winRate >= 50 ? 'positive' : 'negative'}`}>
                          {sessionStats.absorptions.winRate.toFixed(1)}%
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (1m)</span>
                        <span className={`metric-value ${sessionStats.absorptions.avgMove1m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.absorptions.avgMove1m >= 0 ? '+' : ''}{sessionStats.absorptions.avgMove1m.toFixed(2)}
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (5m)</span>
                        <span className={`metric-value ${sessionStats.absorptions.avgMove5m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.absorptions.avgMove5m >= 0 ? '+' : ''}{sessionStats.absorptions.avgMove5m.toFixed(2)}
                        </span>
                      </div>
                    </div>
                    <div className="win-loss">
                      <span className="wins">{sessionStats.absorptions.wins} W</span>
                      <span className="losses">{sessionStats.absorptions.losses} L</span>
                    </div>
                  </div>

                  {/* Stacked Imbalances */}
                  <div className="signal-card">
                    <h3>Stacked Imbalances</h3>
                    <div className="signal-counts">
                      <span className="count total">{sessionStats.stackedImbalances.count} total</span>
                      <span className="count bullish">{sessionStats.stackedImbalances.bullishCount} bullish</span>
                      <span className="count bearish">{sessionStats.stackedImbalances.bearishCount} bearish</span>
                    </div>
                    <div className="signal-metrics">
                      <div className="metric">
                        <span className="metric-label">Win Rate</span>
                        <span className={`metric-value ${sessionStats.stackedImbalances.winRate >= 50 ? 'positive' : 'negative'}`}>
                          {sessionStats.stackedImbalances.winRate.toFixed(1)}%
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (1m)</span>
                        <span className={`metric-value ${sessionStats.stackedImbalances.avgMove1m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.stackedImbalances.avgMove1m >= 0 ? '+' : ''}{sessionStats.stackedImbalances.avgMove1m.toFixed(2)}
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (5m)</span>
                        <span className={`metric-value ${sessionStats.stackedImbalances.avgMove5m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.stackedImbalances.avgMove5m >= 0 ? '+' : ''}{sessionStats.stackedImbalances.avgMove5m.toFixed(2)}
                        </span>
                      </div>
                    </div>
                    <div className="win-loss">
                      <span className="wins">{sessionStats.stackedImbalances.wins} W</span>
                      <span className="losses">{sessionStats.stackedImbalances.losses} L</span>
                    </div>
                  </div>

                  {/* Confluences */}
                  <div className="signal-card confluence-card">
                    <h3>Confluences</h3>
                    <div className="signal-counts">
                      <span className="count total">{sessionStats.confluences.count} total</span>
                      <span className="count bullish">{sessionStats.confluences.bullishCount} bullish</span>
                      <span className="count bearish">{sessionStats.confluences.bearishCount} bearish</span>
                    </div>
                    <div className="signal-metrics">
                      <div className="metric">
                        <span className="metric-label">Win Rate</span>
                        <span className={`metric-value ${sessionStats.confluences.winRate >= 50 ? 'positive' : 'negative'}`}>
                          {sessionStats.confluences.winRate.toFixed(1)}%
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (1m)</span>
                        <span className={`metric-value ${sessionStats.confluences.avgMove1m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.confluences.avgMove1m >= 0 ? '+' : ''}{sessionStats.confluences.avgMove1m.toFixed(2)}
                        </span>
                      </div>
                      <div className="metric">
                        <span className="metric-label">Avg Move (5m)</span>
                        <span className={`metric-value ${sessionStats.confluences.avgMove5m >= 0 ? 'positive' : 'negative'}`}>
                          {sessionStats.confluences.avgMove5m >= 0 ? '+' : ''}{sessionStats.confluences.avgMove5m.toFixed(2)}
                        </span>
                      </div>
                    </div>
                    <div className="win-loss">
                      <span className="wins">{sessionStats.confluences.wins} W</span>
                      <span className="losses">{sessionStats.confluences.losses} L</span>
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="stats-loading">
                <p>Waiting for session data...</p>
                <p className="stats-hint">Statistics will appear after signals are detected</p>
              </div>
            )}
          </div>
        )}
      </div>

      <footer className="footer">
        <div className="legend">
          <span className="legend-item buy">
            <span className="legend-dot"></span>
            BUY AGGRESSION
          </span>
          <span className="legend-item sell">
            <span className="legend-dot"></span>
            SELL AGGRESSION
          </span>
        </div>
        <div className="bubble-count">{bubbles.length} bubbles</div>
      </footer>
    </div>
  );
}

export default App;
