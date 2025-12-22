import { useEffect, RefObject } from 'react';

interface Bubble {
  id: string;
  price: number;
  size: number;
  side: 'buy' | 'sell';
  timestamp: number;
  x: number;
  opacity: number;
  isSignificantImbalance?: boolean; // Imbalance > 15%
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

interface VolumeProfileLevel {
  price: number;
  buyVolume: number;
  sellVolume: number;
  totalVolume: number;
}

interface BubbleRendererProps {
  bubbles: Bubble[];
  priceRange: { min: number; max: number } | null;
  canvasRef: RefObject<HTMLCanvasElement>;
  cvdHistory: CVDPoint[];
  cvdRange: { min: number; max: number };
  currentCVD: number;
  zeroCrosses: ZeroCross[];
  onClick?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
  volumeProfile: Map<number, VolumeProfileLevel>;
}

// Colors matching trading aesthetic
const COLORS = {
  buy: {
    fill: 'rgba(0, 230, 118, 0.6)',
    stroke: 'rgba(0, 230, 118, 0.9)',
    glow: 'rgba(0, 230, 118, 0.3)'
  },
  sell: {
    fill: 'rgba(255, 82, 82, 0.6)',
    stroke: 'rgba(255, 82, 82, 0.9)',
    glow: 'rgba(255, 82, 82, 0.3)'
  },
  neutral: {
    fill: 'rgba(158, 158, 158, 0.5)',
    stroke: 'rgba(158, 158, 158, 0.8)',
    glow: 'rgba(158, 158, 158, 0.3)'
  },
  cvd: {
    positive: 'rgba(0, 230, 118, 0.8)',
    negative: 'rgba(255, 82, 82, 0.8)',
    zero: 'rgba(255, 255, 255, 0.3)'
  },
  grid: 'rgba(255, 255, 255, 0.05)',
  gridText: 'rgba(255, 255, 255, 0.4)',
  background: '#0a0a0a'
};

// Size scaling - linear scaling by order size for orderflow visualization
const MIN_BUBBLE_RADIUS = 5;     // Minimum visible size
const MAX_BUBBLE_RADIUS = 80;    // Maximum size for extreme volume
const SIZE_SCALE_FACTOR = 0.12;  // Linear scaling: 100 contracts = 12px, 300 = 36px, 500 = 60px

export function BubbleRenderer({
  bubbles,
  priceRange,
  canvasRef,
  cvdHistory,
  cvdRange,
  currentCVD,
  zeroCrosses,
  onClick,
  volumeProfile
}: BubbleRendererProps) {
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Set up high DPI canvas
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    // Clear canvas
    ctx.fillStyle = COLORS.background;
    ctx.fillRect(0, 0, rect.width, rect.height);

    if (!priceRange || bubbles.length === 0) {
      // Draw placeholder text
      ctx.fillStyle = 'rgba(255, 255, 255, 0.2)';
      ctx.font = '14px "JetBrains Mono", monospace';
      ctx.textAlign = 'center';
      ctx.fillText('Waiting for trades...', rect.width / 2, rect.height / 2);
      return;
    }

    const { min: priceMin, max: priceMax } = priceRange;
    const priceSpan = priceMax - priceMin;

    // Draw price grid
    drawPriceGrid(ctx, rect.width, rect.height, priceMin, priceMax);

    // Draw CVD line chart at bottom
    const cvdPanelHeight = 80; // Bottom panel for CVD
    drawCVDChart(ctx, cvdHistory, cvdRange, currentCVD, rect.width, rect.height, cvdPanelHeight);

    // Draw zero-cross vertical markers (BEFORE bubbles so they're in background)
    const mainChartHeight = rect.height - cvdPanelHeight;
    zeroCrosses.forEach(cross => {
      const x = cross.x * rect.width;
      const color = cross.direction === 'bullish'
        ? 'rgba(0, 230, 118, 0.3)'
        : 'rgba(255, 82, 82, 0.3)';

      // Vertical line (avoid volume profile on left)
      ctx.strokeStyle = color;
      ctx.lineWidth = 3;
      ctx.setLineDash([8, 4]);
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, mainChartHeight);
      ctx.stroke();
      ctx.setLineDash([]);

      // Label at top
      ctx.fillStyle = cross.direction === 'bullish'
        ? COLORS.buy.stroke
        : COLORS.sell.stroke;
      ctx.font = 'bold 10px "JetBrains Mono", monospace';
      ctx.textAlign = 'center';
      ctx.fillText(
        cross.direction === 'bullish' ? '↑ FLIP' : '↓ FLIP',
        x,
        15
      );

      // Price label (if available)
      if (cross.price !== undefined) {
        ctx.font = 'bold 9px "JetBrains Mono", monospace';
        ctx.fillStyle = 'rgba(255, 255, 255, 0.6)';
        ctx.fillText(
          cross.price.toFixed(2),
          x,
          30
        );
      }
    });

    // Draw bubbles
    bubbles.forEach(bubble => {
      const x = bubble.x * rect.width;
      const y = rect.height - ((bubble.price - priceMin) / priceSpan) * rect.height;

      // Scale radius linearly based on order size (aggression)
      const radius = Math.min(
        MAX_BUBBLE_RADIUS,
        Math.max(MIN_BUBBLE_RADIUS, bubble.size * SIZE_SCALE_FACTOR)
      );

      // Use grey for insignificant imbalances, colored for significant ones
      const isSignificant = bubble.isSignificantImbalance;
      const colors = isSignificant
        ? (bubble.side === 'buy' ? COLORS.buy : COLORS.sell)
        : COLORS.neutral;
      const opacity = bubble.opacity;

      // Enhanced glow effects based on trade size (Fabio Valentini tiers)
      if (bubble.size >= 200) {
        // 200+ contracts: Institutional - Massive pulsing glow
        const gradient = ctx.createRadialGradient(x, y, 0, x, y, radius * 3);
        gradient.addColorStop(0, colors.glow.replace('0.3', `${0.6 * opacity}`));
        gradient.addColorStop(0.5, colors.glow.replace('0.3', `${0.3 * opacity}`));
        gradient.addColorStop(1, 'transparent');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(x, y, radius * 3, 0, Math.PI * 2);
        ctx.fill();
      } else if (bubble.size >= 100) {
        // 100-200 contracts: Large institutional - Strong glow
        const gradient = ctx.createRadialGradient(x, y, 0, x, y, radius * 2.5);
        gradient.addColorStop(0, colors.glow.replace('0.3', `${0.5 * opacity}`));
        gradient.addColorStop(1, 'transparent');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(x, y, radius * 2.5, 0, Math.PI * 2);
        ctx.fill();
      } else if (bubble.size >= 50) {
        // 50-100 contracts: Medium institutional - Enhanced glow
        const gradient = ctx.createRadialGradient(x, y, 0, x, y, radius * 2.2);
        gradient.addColorStop(0, colors.glow.replace('0.3', `${0.4 * opacity}`));
        gradient.addColorStop(1, 'transparent');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(x, y, radius * 2.2, 0, Math.PI * 2);
        ctx.fill();
      } else if (bubble.size >= 10) {
        // 10-50 contracts: Standard glow
        const gradient = ctx.createRadialGradient(x, y, 0, x, y, radius * 2);
        gradient.addColorStop(0, colors.glow.replace('0.3', `${0.3 * opacity}`));
        gradient.addColorStop(1, 'transparent');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(x, y, radius * 2, 0, Math.PI * 2);
        ctx.fill();
      }

      // Draw main bubble
      ctx.globalAlpha = opacity;

      // Fill
      ctx.fillStyle = colors.fill;
      ctx.beginPath();
      ctx.arc(x, y, radius, 0, Math.PI * 2);
      ctx.fill();

      // Stroke
      ctx.strokeStyle = colors.stroke;
      ctx.lineWidth = 1.5;
      ctx.stroke();

      // Size label for large trades
      if (bubble.size >= 5 && radius > 15) {
        ctx.fillStyle = `rgba(255, 255, 255, ${0.9 * opacity})`;
        ctx.font = `bold ${Math.min(radius * 0.6, 14)}px "JetBrains Mono", monospace`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(bubble.size.toString(), x, y);
      }

      ctx.globalAlpha = 1;
    });

    // Draw current price line
    const lastBubble = bubbles[bubbles.length - 1];
    if (lastBubble) {
      const lastY = rect.height - ((lastBubble.price - priceMin) / priceSpan) * rect.height;

      ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(120, lastY); // Start after volume profile (120px)
      ctx.lineTo(rect.width, lastY);
      ctx.stroke();
      ctx.setLineDash([]);

      // Price label
      ctx.fillStyle = lastBubble.side === 'buy' ? COLORS.buy.stroke : COLORS.sell.stroke;
      ctx.font = 'bold 11px "JetBrains Mono", monospace';
      ctx.textAlign = 'right';
      ctx.fillText(lastBubble.price.toFixed(2), rect.width - 8, lastY - 8);
    }

    // Draw volume profile on LEFT edge (AFTER bubbles so it's on top)
    drawVolumeProfileEnhanced(ctx, volumeProfile, rect.width, mainChartHeight, priceMin, priceMax);

  }, [bubbles, priceRange, canvasRef, cvdHistory, cvdRange, currentCVD, zeroCrosses, volumeProfile]);

  return (
    <canvas
      ref={canvasRef}
      className="bubble-canvas"
      onClick={onClick}
      style={{
        width: '100%',
        height: '100%',
        display: 'block',
        cursor: onClick ? 'pointer' : 'default'
      }}
    />
  );
}

function drawPriceGrid(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  priceMin: number,
  priceMax: number
) {
  const priceSpan = priceMax - priceMin;
  
  // Calculate nice price intervals
  const rawInterval = priceSpan / 8;
  const magnitude = Math.pow(10, Math.floor(Math.log10(rawInterval)));
  const normalized = rawInterval / magnitude;
  
  let interval: number;
  if (normalized < 1.5) interval = magnitude;
  else if (normalized < 3) interval = 2 * magnitude;
  else if (normalized < 7) interval = 5 * magnitude;
  else interval = 10 * magnitude;

  // Round to nice tick values
  const startPrice = Math.ceil(priceMin / interval) * interval;

  ctx.strokeStyle = COLORS.grid;
  ctx.lineWidth = 1;
  ctx.fillStyle = 'rgba(255, 255, 255, 0.9)'; // Much brighter (was 0.4)
  ctx.font = 'bold 13px "JetBrains Mono", monospace'; // Bigger and bold (was 10px)
  ctx.textAlign = 'right';

  for (let price = startPrice; price <= priceMax; price += interval) {
    const y = height - ((price - priceMin) / priceSpan) * height;

    // Grid line (start after volume profile on left)
    ctx.beginPath();
    ctx.moveTo(120, y); // Start after volume profile (120px wide)
    ctx.lineTo(width - 60, y); // End before price labels on right
    ctx.stroke();

    // Price label
    ctx.fillText(price.toFixed(2), width - 8, y + 4);
  }
}

// CVD Line Chart - Shows cumulative volume delta
function drawCVDChart(
  ctx: CanvasRenderingContext2D,
  cvdHistory: CVDPoint[],
  cvdRange: { min: number; max: number },
  currentCVD: number,
  width: number,
  height: number,
  panelHeight: number
) {
  if (cvdHistory.length < 2) return;

  const panelY = height - panelHeight;
  const cvdSpan = Math.max(Math.abs(cvdRange.max), Math.abs(cvdRange.min), 100);

  // Draw panel background
  ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
  ctx.fillRect(0, panelY, width, panelHeight);

  // Draw zero line
  const zeroY = panelY + panelHeight / 2;
  ctx.strokeStyle = COLORS.cvd.zero;
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 2]);
  ctx.beginPath();
  ctx.moveTo(0, zeroY);
  ctx.lineTo(width, zeroY);
  ctx.stroke();
  ctx.setLineDash([]);

  // Draw CVD line with smoothing
  if (cvdHistory.length > 0) {
    ctx.beginPath();
    ctx.strokeStyle = currentCVD >= 0 ? COLORS.cvd.positive : COLORS.cvd.negative;
    ctx.lineWidth = 2.5;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    // Start at first point
    const firstX = cvdHistory[0].x * width;
    const firstY = zeroY - (cvdHistory[0].value / cvdSpan) * (panelHeight / 2);
    ctx.moveTo(firstX, firstY);

    // Draw smooth curve through points
    for (let i = 1; i < cvdHistory.length; i++) {
      const x = cvdHistory[i].x * width;
      const cvdY = zeroY - (cvdHistory[i].value / cvdSpan) * (panelHeight / 2);
      ctx.lineTo(x, cvdY);
    }

    ctx.stroke();
  }

  // Fill area under CVD line
  if (cvdHistory.length > 0) {
    const lastPoint = cvdHistory[cvdHistory.length - 1];
    const lastX = lastPoint.x * width;
    ctx.lineTo(lastX, zeroY);
    ctx.lineTo(cvdHistory[0].x * width, zeroY);
    ctx.closePath();

    const fillColor = currentCVD >= 0
      ? COLORS.cvd.positive.replace('0.8', '0.2')
      : COLORS.cvd.negative.replace('0.8', '0.2');
    ctx.fillStyle = fillColor;
    ctx.fill();
  }

  // Draw CVD value label
  ctx.fillStyle = currentCVD >= 0 ? COLORS.cvd.positive : COLORS.cvd.negative;
  ctx.font = 'bold 12px "JetBrains Mono", monospace';
  ctx.textAlign = 'left';
  ctx.fillText(`CVD: ${currentCVD > 0 ? '+' : ''}${currentCVD.toFixed(0)}`, 10, panelY + 15);

  // Draw "ZERO" label
  ctx.fillStyle = COLORS.cvd.zero;
  ctx.font = '9px "JetBrains Mono", monospace';
  ctx.textAlign = 'right';
  ctx.fillText('ZERO', width - 10, zeroY - 3);
}

// Enhanced Volume Profile with POC and LVN detection
function drawVolumeProfileEnhanced(
  ctx: CanvasRenderingContext2D,
  volumeProfile: Map<number, VolumeProfileLevel>,
  width: number,
  height: number,
  priceMin: number,
  priceMax: number
) {
  if (volumeProfile.size === 0) return;

  const priceSpan = priceMax - priceMin;
  const profileWidth = 120; // Width of volume profile sidebar
  const profileX = 0; // All the way on the left edge

  // Bucket size for aggregation (larger = cleaner profile)
  // Use 1.0 for most cases, or scale based on price range
  const bucketSize = priceSpan > 50 ? 2.0 : 1.0;

  // Aggregate volume into larger buckets
  const bucketMap = new Map<number, VolumeProfileLevel>();

  volumeProfile.forEach((level) => {
    if (level.price < priceMin || level.price > priceMax) return;

    // Round to bucket
    const bucketPrice = Math.floor(level.price / bucketSize) * bucketSize;
    const existing = bucketMap.get(bucketPrice);

    if (existing) {
      bucketMap.set(bucketPrice, {
        price: bucketPrice,
        buyVolume: existing.buyVolume + level.buyVolume,
        sellVolume: existing.sellVolume + level.sellVolume,
        totalVolume: existing.totalVolume + level.totalVolume
      });
    } else {
      bucketMap.set(bucketPrice, {
        price: bucketPrice,
        buyVolume: level.buyVolume,
        sellVolume: level.sellVolume,
        totalVolume: level.totalVolume
      });
    }
  });

  // Convert to array and sort
  const levels = Array.from(bucketMap.values()).sort((a, b) => a.price - b.price);
  if (levels.length === 0) return;

  // Find max volume for scaling
  const maxVolume = Math.max(...levels.map(l => l.totalVolume));

  // Calculate POC (Point of Control) - highest volume level
  const poc = levels.reduce((max, level) =>
    level.totalVolume > max.totalVolume ? level : max
  );

  // Calculate VAH and VAL (Value Area High/Low) - 70% of volume
  const totalVolume = levels.reduce((sum, l) => sum + l.totalVolume, 0);
  const targetVolume = totalVolume * 0.70;

  // Start from POC and expand up/down to capture 70% of volume
  const pocIndex = levels.findIndex(l => l === poc);
  let valueAreaVolume = poc.totalVolume;
  let vah = poc;
  let val = poc;
  let upIndex = pocIndex + 1;
  let downIndex = pocIndex - 1;

  while (valueAreaVolume < targetVolume && (upIndex < levels.length || downIndex >= 0)) {
    const upVolume = upIndex < levels.length ? levels[upIndex].totalVolume : 0;
    const downVolume = downIndex >= 0 ? levels[downIndex].totalVolume : 0;

    if (upVolume >= downVolume && upIndex < levels.length) {
      valueAreaVolume += upVolume;
      vah = levels[upIndex];
      upIndex++;
    } else if (downIndex >= 0) {
      valueAreaVolume += downVolume;
      val = levels[downIndex];
      downIndex--;
    } else {
      break;
    }
  }

  // Calculate LVNs (Low Volume Nodes) - levels with < 30% of average volume
  const avgVolume = levels.reduce((sum, l) => sum + l.totalVolume, 0) / levels.length;
  const lvnThreshold = avgVolume * 0.3;
  const lvnCandidates = levels.filter(l => l.totalVolume < lvnThreshold && l.totalVolume > 0);

  // Group consecutive LVNs into zones (merge LVNs within 3 price points)
  const lvnZones: VolumeProfileLevel[] = [];
  let currentZone: VolumeProfileLevel[] = [];

  lvnCandidates.forEach((lvn) => {
    if (currentZone.length === 0) {
      currentZone.push(lvn);
    } else {
      const lastInZone = currentZone[currentZone.length - 1];
      if (Math.abs(lvn.price - lastInZone.price) <= 3.0) {
        // Within 3 points - add to current zone
        currentZone.push(lvn);
      } else {
        // New zone - save current and start new
        const midPrice = currentZone.reduce((sum, l) => sum + l.price, 0) / currentZone.length;
        lvnZones.push({ ...currentZone[0], price: midPrice });
        currentZone = [lvn];
      }
    }
  });

  // Add final zone
  if (currentZone.length > 0) {
    const midPrice = currentZone.reduce((sum, l) => sum + l.price, 0) / currentZone.length;
    lvnZones.push({ ...currentZone[0], price: midPrice });
  }

  const lvns = lvnZones;

  // Draw semi-transparent background
  ctx.fillStyle = 'rgba(0, 0, 0, 0.3)';
  ctx.fillRect(profileX, 0, profileWidth, height);

  // Draw volume bars
  levels.forEach(level => {
    const y = height - ((level.price - priceMin) / priceSpan) * height;
    const barWidth = (level.totalVolume / maxVolume) * profileWidth;

    // Calculate buy/sell proportion
    const buyProportion = level.buyVolume / level.totalVolume;
    const sellProportion = level.sellVolume / level.totalVolume;

    // Bar height based on bucket size (shows clean bars)
    const barHeight = Math.max(3, (bucketSize / priceSpan) * height * 0.95);

    // Draw buy volume (left side, green)
    const buyWidth = barWidth * buyProportion;
    ctx.fillStyle = level === poc
      ? 'rgba(0, 230, 118, 0.9)' // POC is brighter
      : 'rgba(0, 230, 118, 0.5)';
    ctx.fillRect(profileX, y - barHeight / 2, buyWidth, barHeight);

    // Draw sell volume (right side, red)
    const sellWidth = barWidth * sellProportion;
    ctx.fillStyle = level === poc
      ? 'rgba(255, 82, 82, 0.9)' // POC is brighter
      : 'rgba(255, 82, 82, 0.5)';
    ctx.fillRect(profileX + buyWidth, y - barHeight / 2, sellWidth, barHeight);

    // Highlight POC (Point of Control)
    if (level === poc) {
      ctx.strokeStyle = 'rgba(0, 200, 255, 0.9)'; // Cyan border
      ctx.lineWidth = 2;
      ctx.strokeRect(profileX - 2, y - barHeight / 2 - 1, barWidth + 4, barHeight + 2);

      // POC horizontal line across chart
      ctx.strokeStyle = 'rgba(0, 200, 255, 0.6)'; // Cyan dashed line
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(profileX + profileWidth, y);
      ctx.lineTo(width - 60, y);
      ctx.stroke();
      ctx.setLineDash([]);

      // POC label
      ctx.fillStyle = 'rgba(0, 200, 255, 1)';
      ctx.font = 'bold 8px "JetBrains Mono", monospace';
      ctx.textAlign = 'left';
      ctx.fillText('POC', profileX + profileWidth + 5, y + 3);
    }

    // Mark LVNs (Low Volume Nodes)
    if (lvns.includes(level)) {
      ctx.strokeStyle = 'rgba(255, 140, 0, 0.5)'; // Orange dashed line
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(profileX + profileWidth, y); // Start after volume profile
      ctx.lineTo(width - 60, y); // Extend to price labels
      ctx.stroke();
      ctx.setLineDash([]);

      // LVN label on volume profile
      ctx.fillStyle = 'rgba(255, 140, 0, 1)';
      ctx.font = 'bold 8px "JetBrains Mono", monospace';
      ctx.textAlign = 'left';
      ctx.fillText('LVN', profileX + profileWidth + 5, y + 3);
    }
  });

  // Draw VAH (Value Area High) line
  const vahY = height - ((vah.price - priceMin) / priceSpan) * height;
  ctx.strokeStyle = 'rgba(138, 43, 226, 0.7)'; // Purple for VAH
  ctx.lineWidth = 2;
  ctx.setLineDash([8, 4]);
  ctx.beginPath();
  ctx.moveTo(profileX + profileWidth, vahY);
  ctx.lineTo(width - 60, vahY);
  ctx.stroke();
  ctx.setLineDash([]);

  // VAH label
  ctx.fillStyle = 'rgba(138, 43, 226, 1)';
  ctx.font = 'bold 9px "JetBrains Mono", monospace';
  ctx.textAlign = 'left';
  ctx.fillText('VAH', profileX + profileWidth + 5, vahY - 5);

  // Draw VAL (Value Area Low) line
  const valY = height - ((val.price - priceMin) / priceSpan) * height;
  ctx.strokeStyle = 'rgba(138, 43, 226, 0.7)'; // Purple for VAL
  ctx.lineWidth = 2;
  ctx.setLineDash([8, 4]);
  ctx.beginPath();
  ctx.moveTo(profileX + profileWidth, valY);
  ctx.lineTo(width - 60, valY);
  ctx.stroke();
  ctx.setLineDash([]);

  // VAL label
  ctx.fillStyle = 'rgba(138, 43, 226, 1)';
  ctx.font = 'bold 9px "JetBrains Mono", monospace';
  ctx.textAlign = 'left';
  ctx.fillText('VAL', profileX + profileWidth + 5, valY + 12);

  // Draw profile border
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.2)';
  ctx.lineWidth = 1;
  ctx.strokeRect(profileX, 0, profileWidth, height);

  // Draw title
  ctx.fillStyle = 'rgba(255, 255, 255, 0.7)';
  ctx.font = 'bold 10px "JetBrains Mono", monospace';
  ctx.textAlign = 'center';
  ctx.fillText('VOLUME PROFILE', profileX + profileWidth / 2, 15);

  // Draw legend
  const legendY = height - 60;
  ctx.font = '8px "JetBrains Mono", monospace';
  ctx.textAlign = 'left';

  ctx.fillStyle = 'rgba(255, 215, 0, 1)';
  ctx.fillText('POC = High Vol', profileX + 5, legendY);

  ctx.fillStyle = 'rgba(255, 140, 0, 0.8)';
  ctx.fillText('LVN = Low Vol', profileX + 5, legendY + 12);
}

// Optional: Volume Profile on right side (legacy, kept for reference)
export function drawVolumeProfile(
  ctx: CanvasRenderingContext2D,
  bubbles: Bubble[],
  width: number,
  height: number,
  priceMin: number,
  priceMax: number
) {
  const priceSpan = priceMax - priceMin;
  const bucketSize = priceSpan / 50; // 50 price buckets
  const volumeByPrice = new Map<number, { buy: number; sell: number }>();

  // Aggregate volume by price bucket
  bubbles.forEach(bubble => {
    const bucket = Math.floor((bubble.price - priceMin) / bucketSize);
    const existing = volumeByPrice.get(bucket) || { buy: 0, sell: 0 };
    if (bubble.side === 'buy') {
      existing.buy += bubble.size;
    } else {
      existing.sell += bubble.size;
    }
    volumeByPrice.set(bucket, existing);
  });

  // Find max volume for scaling
  let maxVol = 0;
  volumeByPrice.forEach(v => {
    maxVol = Math.max(maxVol, v.buy + v.sell);
  });

  if (maxVol === 0) return;

  const barHeight = height / 50;
  const maxBarWidth = 50;

  volumeByPrice.forEach((vol, bucket) => {
    const y = height - (bucket + 1) * barHeight;
    const totalWidth = ((vol.buy + vol.sell) / maxVol) * maxBarWidth;
    const buyWidth = (vol.buy / (vol.buy + vol.sell)) * totalWidth;
    const sellWidth = totalWidth - buyWidth;

    // Buy volume (green, left side of profile)
    ctx.fillStyle = COLORS.buy.fill;
    ctx.fillRect(width - 50 - buyWidth, y, buyWidth, barHeight - 1);

    // Sell volume (red, right side)
    ctx.fillStyle = COLORS.sell.fill;
    ctx.fillRect(width - 50, y, sellWidth, barHeight - 1);
  });
}
