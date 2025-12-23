import { useEffect, useState, useCallback, useRef } from 'react';
import { RustWebSocket, WsMessage } from './websocket';
import { StatsCharts } from './StatsCharts';

interface Signal {
  id: string;
  session_id: string | null;
  created_at: string;
  timestamp: number;
  signal_type: string;
  direction: string;
  price: number;
  price_after_1m: number | null;
  price_after_5m: number | null;
  outcome: string | null;
}

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

interface Session {
  id: string;
  started_at: string;
  ended_at: string | null;
  mode: string;
  symbols: string[];
  session_high: number | null;
  session_low: number | null;
  total_volume: number | null;
}

interface LiveSessionStats {
  sessionStart: number;
  currentPrice: number;
  sessionHigh: number;
  sessionLow: number;
  totalVolume: number;
}

interface StatsPageProps {
  onClose: () => void;
}

export function StatsPage({ onClose }: StatsPageProps) {
  const [signals, setSignals] = useState<Signal[]>([]);
  const [stats, setStats] = useState<AggregateStats | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'overview' | 'signals' | 'sessions'>('overview');

  // Filters
  const [signalTypeFilter, setSignalTypeFilter] = useState<string>('');
  const [directionFilter, setDirectionFilter] = useState<string>('');
  const [outcomeFilter, setOutcomeFilter] = useState<string>('');
  const [startDate, setStartDate] = useState<string>('');
  const [endDate, setEndDate] = useState<string>('');

  // Live session stats from WebSocket
  const [liveStats, setLiveStats] = useState<LiveSessionStats | null>(null);
  const wsRef = useRef<RustWebSocket | null>(null);

  // WebSocket connection for live stats
  useEffect(() => {
    const ws = new RustWebSocket();
    wsRef.current = ws;

    ws.onMessage((message: WsMessage) => {
      if (message.type === 'SessionStats') {
        setLiveStats({
          sessionStart: message.sessionStart,
          currentPrice: message.currentPrice,
          sessionHigh: message.sessionHigh,
          sessionLow: message.sessionLow,
          totalVolume: message.totalVolume,
        });
      }
    });

    ws.connect().catch(console.error);

    return () => {
      ws.disconnect();
    };
  }, []);

  const fetchData = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);

      // Build query params
      const params = new URLSearchParams();
      params.append('limit', '100');
      if (signalTypeFilter) params.append('signal_type', signalTypeFilter);
      if (directionFilter) params.append('direction', directionFilter);
      if (outcomeFilter) params.append('outcome', outcomeFilter);
      if (startDate) params.append('start_date', new Date(startDate).toISOString());
      if (endDate) params.append('end_date', new Date(endDate).toISOString());

      const [signalsRes, statsRes, sessionsRes] = await Promise.all([
        fetch(`/api/signals?${params.toString()}`),
        fetch('/api/stats'),
        fetch('/api/sessions?limit=10'),
      ]);

      if (!signalsRes.ok || !statsRes.ok || !sessionsRes.ok) {
        throw new Error('Failed to fetch data');
      }

      const signalsData = await signalsRes.json();
      const statsData = await statsRes.json();
      const sessionsData = await sessionsRes.json();

      setSignals(signalsData.signals || []);
      setStats(statsData);
      setSessions(sessionsData.sessions || []);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Unknown error');
    } finally {
      setLoading(false);
    }
  }, [signalTypeFilter, directionFilter, outcomeFilter, startDate, endDate]);

  useEffect(() => {
    fetchData();
    // Refresh every 30 seconds
    const interval = setInterval(fetchData, 30000);
    return () => clearInterval(interval);
  }, [fetchData]);

  const handleExport = (format: 'csv' | 'json') => {
    const params = new URLSearchParams();
    params.append('format', format);
    if (signalTypeFilter) params.append('signal_type', signalTypeFilter);
    if (directionFilter) params.append('direction', directionFilter);
    if (outcomeFilter) params.append('outcome', outcomeFilter);
    if (startDate) params.append('start_date', new Date(startDate).toISOString());
    if (endDate) params.append('end_date', new Date(endDate).toISOString());

    window.open(`/api/signals/export?${params.toString()}`, '_blank');
  };

  const formatTime = (timestamp: number) => {
    return new Date(timestamp).toLocaleString();
  };

  const formatPrice = (price: number | null) => {
    return price !== null ? price.toFixed(2) : '-';
  };

  const getOutcomeClass = (outcome: string | null) => {
    if (!outcome) return '';
    if (outcome === 'win') return 'outcome-win';
    if (outcome === 'loss') return 'outcome-loss';
    return 'outcome-breakeven';
  };

  const signalTypes = ['delta_flip', 'absorption', 'stacked_imbalance', 'confluence'];

  return (
    <div className="stats-page">
      <div className="stats-page-header">
        <h1>Historical Statistics</h1>
        <div className="stats-page-actions">
          <button className="export-btn" onClick={() => handleExport('csv')}>
            Export CSV
          </button>
          <button className="export-btn" onClick={() => handleExport('json')}>
            Export JSON
          </button>
          <button className="refresh-btn" onClick={fetchData} disabled={loading}>
            {loading ? 'Loading...' : 'Refresh'}
          </button>
          <button className="close-btn" onClick={onClose}>
            Back to Chart
          </button>
        </div>
      </div>

      {/* Live session stats banner */}
      {liveStats && (
        <div className="live-stats-banner">
          <span className="live-indicator">LIVE</span>
          <span>Price: {liveStats.currentPrice.toFixed(2)}</span>
          <span>High: {liveStats.sessionHigh.toFixed(2)}</span>
          <span>Low: {liveStats.sessionLow.toFixed(2)}</span>
          <span>Volume: {liveStats.totalVolume.toLocaleString()}</span>
        </div>
      )}

      {error && <div className="stats-error">{error}</div>}

      <div className="stats-tabs">
        <button
          className={`tab ${activeTab === 'overview' ? 'active' : ''}`}
          onClick={() => setActiveTab('overview')}
        >
          Overview
        </button>
        <button
          className={`tab ${activeTab === 'signals' ? 'active' : ''}`}
          onClick={() => setActiveTab('signals')}
        >
          Signals ({signals.length})
        </button>
        <button
          className={`tab ${activeTab === 'sessions' ? 'active' : ''}`}
          onClick={() => setActiveTab('sessions')}
        >
          Sessions ({sessions.length})
        </button>
      </div>

      {activeTab === 'overview' && stats && (
        <div className="stats-overview-page">
          <StatsCharts stats={stats} />
          <div className="stats-summary-cards">
            <div className="summary-card">
              <div className="summary-value">{stats.total_signals}</div>
              <div className="summary-label">Total Signals</div>
            </div>
            <div className="summary-card bullish">
              <div className="summary-value">{stats.by_direction.bullish}</div>
              <div className="summary-label">Bullish</div>
            </div>
            <div className="summary-card bearish">
              <div className="summary-value">{stats.by_direction.bearish}</div>
              <div className="summary-label">Bearish</div>
            </div>
          </div>

          <div className="stats-by-type">
            <h3>By Signal Type</h3>
            <div className="type-cards">
              {signalTypes.map(type => {
                const typeStats = stats.by_type[type];
                if (!typeStats) return null;
                return (
                  <div key={type} className="type-card">
                    <h4>{type.replace('_', ' ').toUpperCase()}</h4>
                    <div className="type-stats">
                      <div className="stat-row">
                        <span>Count:</span>
                        <span>{typeStats.count}</span>
                      </div>
                      <div className="stat-row">
                        <span>Wins:</span>
                        <span className="win">{typeStats.wins}</span>
                      </div>
                      <div className="stat-row">
                        <span>Losses:</span>
                        <span className="loss">{typeStats.losses}</span>
                      </div>
                      <div className="stat-row">
                        <span>Win Rate:</span>
                        <span className={typeStats.win_rate >= 50 ? 'win' : 'loss'}>
                          {typeStats.win_rate.toFixed(1)}%
                        </span>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {activeTab === 'signals' && (
        <div className="signals-tab">
          <div className="filters">
            <select
              value={signalTypeFilter}
              onChange={(e) => setSignalTypeFilter(e.target.value)}
            >
              <option value="">All Types</option>
              {signalTypes.map(type => (
                <option key={type} value={type}>{type.replace('_', ' ')}</option>
              ))}
            </select>
            <select
              value={directionFilter}
              onChange={(e) => setDirectionFilter(e.target.value)}
            >
              <option value="">All Directions</option>
              <option value="bullish">Bullish</option>
              <option value="bearish">Bearish</option>
            </select>
            <select
              value={outcomeFilter}
              onChange={(e) => setOutcomeFilter(e.target.value)}
            >
              <option value="">All Outcomes</option>
              <option value="win">Win</option>
              <option value="loss">Loss</option>
              <option value="breakeven">Breakeven</option>
            </select>
            <input
              type="date"
              value={startDate}
              onChange={(e) => setStartDate(e.target.value)}
              placeholder="Start Date"
              className="date-input"
            />
            <input
              type="date"
              value={endDate}
              onChange={(e) => setEndDate(e.target.value)}
              placeholder="End Date"
              className="date-input"
            />
          </div>

          <div className="signals-table-container">
            <table className="signals-table">
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Type</th>
                  <th>Direction</th>
                  <th>Entry Price</th>
                  <th>1m Price</th>
                  <th>5m Price</th>
                  <th>Outcome</th>
                </tr>
              </thead>
              <tbody>
                {signals.map(signal => (
                  <tr key={signal.id} className={signal.direction}>
                    <td>{formatTime(signal.timestamp)}</td>
                    <td>{signal.signal_type.replace('_', ' ')}</td>
                    <td className={signal.direction}>{signal.direction}</td>
                    <td>{formatPrice(signal.price)}</td>
                    <td>{formatPrice(signal.price_after_1m)}</td>
                    <td>{formatPrice(signal.price_after_5m)}</td>
                    <td className={getOutcomeClass(signal.outcome)}>
                      {signal.outcome || 'pending'}
                    </td>
                  </tr>
                ))}
                {signals.length === 0 && (
                  <tr>
                    <td colSpan={7} className="no-data">No signals found</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {activeTab === 'sessions' && (
        <div className="sessions-tab">
          <div className="sessions-list">
            {sessions.map(session => (
              <div key={session.id} className="session-card">
                <div className="session-header">
                  <span className={`mode-badge ${session.mode}`}>{session.mode.toUpperCase()}</span>
                  <span className="session-time">
                    {new Date(session.started_at).toLocaleString()}
                  </span>
                </div>
                <div className="session-details">
                  <div className="detail">
                    <span className="label">Symbols:</span>
                    <span className="value">{session.symbols.join(', ')}</span>
                  </div>
                  {session.session_high && session.session_low && (
                    <div className="detail">
                      <span className="label">Range:</span>
                      <span className="value">
                        {session.session_low.toFixed(2)} - {session.session_high.toFixed(2)}
                      </span>
                    </div>
                  )}
                  {session.total_volume !== null && (
                    <div className="detail">
                      <span className="label">Volume:</span>
                      <span className="value">{session.total_volume.toLocaleString()}</span>
                    </div>
                  )}
                  <div className="detail">
                    <span className="label">Status:</span>
                    <span className={`value ${session.ended_at ? 'ended' : 'active'}`}>
                      {session.ended_at ? 'Ended' : 'Active'}
                    </span>
                  </div>
                </div>
              </div>
            ))}
            {sessions.length === 0 && (
              <div className="no-data">No sessions found</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
