import { useRef, useEffect } from 'react';

interface SignalTypeStats {
  count: number;
  wins: number;
  losses: number;
  breakeven: number;
  win_rate: number;
}

interface AggregateStats {
  total_signals: number;
  by_type: Record<string, SignalTypeStats>;
  by_direction: {
    bullish: number;
    bearish: number;
  };
}

interface StatsChartsProps {
  stats: AggregateStats;
}

const CHART_COLORS = {
  green: '#00e676',
  red: '#ff5252',
  yellow: '#ffc107',
  blue: '#448aff',
  textPrimary: '#ffffff',
  textSecondary: 'rgba(255, 255, 255, 0.7)',
  textMuted: 'rgba(255, 255, 255, 0.4)',
  bgTertiary: '#1a1a1a',
  border: 'rgba(255, 255, 255, 0.08)',
};

const SIGNAL_TYPES = ['delta_flip', 'absorption', 'stacked_imbalance', 'confluence'];

export function StatsCharts({ stats }: StatsChartsProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !stats) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Handle high DPI displays
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    // Clear canvas
    ctx.clearRect(0, 0, rect.width, rect.height);

    drawWinRateChart(ctx, stats, rect.width, rect.height);
  }, [stats]);

  return (
    <div className="stats-charts">
      <h3>Win Rate by Signal Type</h3>
      <canvas ref={canvasRef} className="win-rate-canvas" />
    </div>
  );
}

function drawWinRateChart(
  ctx: CanvasRenderingContext2D,
  stats: AggregateStats,
  width: number,
  height: number
) {
  const padding = { top: 30, right: 20, bottom: 60, left: 50 };
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;

  // Filter to types that have data
  const typesWithData = SIGNAL_TYPES.filter(type => stats.by_type[type]?.count > 0);

  if (typesWithData.length === 0) {
    // Draw "No data" message
    ctx.font = '14px "Space Grotesk", sans-serif';
    ctx.fillStyle = CHART_COLORS.textMuted;
    ctx.textAlign = 'center';
    ctx.fillText('No signal data yet', width / 2, height / 2);
    return;
  }

  const barWidth = Math.min(80, (chartWidth / typesWithData.length) - 20);
  const gap = (chartWidth - (barWidth * typesWithData.length)) / (typesWithData.length + 1);

  // Draw Y-axis
  ctx.strokeStyle = CHART_COLORS.border;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(padding.left, padding.top);
  ctx.lineTo(padding.left, padding.top + chartHeight);
  ctx.stroke();

  // Draw X-axis
  ctx.beginPath();
  ctx.moveTo(padding.left, padding.top + chartHeight);
  ctx.lineTo(padding.left + chartWidth, padding.top + chartHeight);
  ctx.stroke();

  // Draw Y-axis labels (0%, 25%, 50%, 75%, 100%)
  ctx.font = '10px "JetBrains Mono", monospace';
  ctx.fillStyle = CHART_COLORS.textMuted;
  ctx.textAlign = 'right';

  const yLabels = [0, 25, 50, 75, 100];
  yLabels.forEach(percent => {
    const y = padding.top + chartHeight - (chartHeight * percent / 100);
    ctx.fillText(`${percent}%`, padding.left - 8, y + 4);

    // Draw grid line
    ctx.strokeStyle = CHART_COLORS.border;
    ctx.beginPath();
    ctx.moveTo(padding.left, y);
    ctx.lineTo(padding.left + chartWidth, y);
    ctx.stroke();
  });

  // Draw 50% reference line (more prominent)
  ctx.strokeStyle = CHART_COLORS.yellow;
  ctx.setLineDash([5, 5]);
  const fiftyPercentY = padding.top + chartHeight / 2;
  ctx.beginPath();
  ctx.moveTo(padding.left, fiftyPercentY);
  ctx.lineTo(padding.left + chartWidth, fiftyPercentY);
  ctx.stroke();
  ctx.setLineDash([]);

  // Draw bars
  typesWithData.forEach((type, i) => {
    const typeStats = stats.by_type[type];
    if (!typeStats) return;

    const winRate = typeStats.win_rate;
    const barHeight = (winRate / 100) * chartHeight;
    const x = padding.left + gap + i * (barWidth + gap);
    const y = padding.top + chartHeight - barHeight;

    // Choose color based on win rate
    let barColor: string;
    if (winRate >= 60) {
      barColor = CHART_COLORS.green;
    } else if (winRate >= 50) {
      barColor = CHART_COLORS.yellow;
    } else {
      barColor = CHART_COLORS.red;
    }

    // Draw bar with gradient
    const gradient = ctx.createLinearGradient(x, y + barHeight, x, y);
    gradient.addColorStop(0, barColor);
    gradient.addColorStop(1, adjustBrightness(barColor, 0.7));

    ctx.fillStyle = gradient;
    roundedRect(ctx, x, y, barWidth, barHeight, 4);
    ctx.fill();

    // Draw bar border
    ctx.strokeStyle = barColor;
    ctx.lineWidth = 2;
    roundedRect(ctx, x, y, barWidth, barHeight, 4);
    ctx.stroke();

    // Draw win rate label above bar
    ctx.font = 'bold 14px "JetBrains Mono", monospace';
    ctx.fillStyle = barColor;
    ctx.textAlign = 'center';
    ctx.fillText(`${winRate.toFixed(0)}%`, x + barWidth / 2, y - 8);

    // Draw type label below
    ctx.font = '10px "JetBrains Mono", monospace';
    ctx.fillStyle = CHART_COLORS.textSecondary;
    const label = formatTypeName(type);
    ctx.fillText(label, x + barWidth / 2, padding.top + chartHeight + 20);

    // Draw count below label
    ctx.font = '9px "JetBrains Mono", monospace';
    ctx.fillStyle = CHART_COLORS.textMuted;
    ctx.fillText(`(${typeStats.count})`, x + barWidth / 2, padding.top + chartHeight + 34);

    // Draw W/L breakdown
    ctx.font = '9px "JetBrains Mono", monospace';
    const wlText = `${typeStats.wins}W / ${typeStats.losses}L`;
    ctx.fillStyle = CHART_COLORS.textMuted;
    ctx.fillText(wlText, x + barWidth / 2, padding.top + chartHeight + 48);
  });
}

function roundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
) {
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.lineTo(x + width - radius, y);
  ctx.quadraticCurveTo(x + width, y, x + width, y + radius);
  ctx.lineTo(x + width, y + height);
  ctx.lineTo(x, y + height);
  ctx.lineTo(x, y + radius);
  ctx.quadraticCurveTo(x, y, x + radius, y);
  ctx.closePath();
}

function adjustBrightness(hex: string, factor: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);

  return `rgb(${Math.round(r * factor)}, ${Math.round(g * factor)}, ${Math.round(b * factor)})`;
}

function formatTypeName(type: string): string {
  return type
    .split('_')
    .map(word => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}
