import React, { useEffect, useState } from 'react';
import { api, ActiveSession } from '../../api';
import Button from '../../components/Button';

function formatDate(d: string) {
  return new Date(d).toLocaleString(undefined, {
    month: 'short', day: 'numeric', year: 'numeric',
    hour: '2-digit', minute: '2-digit',
  });
}

function parseUA(ua?: string) {
  if (!ua) return 'Unknown device';
  if (ua.includes('iPhone') || ua.includes('Android')) return 'Mobile browser';
  if (ua.includes('Chrome')) return 'Chrome';
  if (ua.includes('Firefox')) return 'Firefox';
  if (ua.includes('Safari')) return 'Safari';
  return 'Browser';
}

function SessionIcon({ current }: { current: boolean }) {
  return (
    <div
      className="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0"
      style={{
        background: current ? '#22c55e1a' : '#161616',
        border: current ? '1px solid #22c55e33' : '1px solid #1e1e1e',
      }}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <rect x="2" y="3" width="12" height="8" rx="1.5" stroke={current ? '#22c55e' : '#6b7280'} strokeWidth="1.3" />
        <path d="M5 14H11" stroke={current ? '#22c55e' : '#6b7280'} strokeWidth="1.3" strokeLinecap="round" />
        <path d="M8 11V14" stroke={current ? '#22c55e' : '#6b7280'} strokeWidth="1.3" strokeLinecap="round" />
      </svg>
    </div>
  );
}

export default function SessionsTab() {
  const [sessions, setSessions] = useState<ActiveSession[]>([]);
  const [loading, setLoading] = useState(true);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [loggingOutAll, setLoggingOutAll] = useState(false);
  const [error, setError] = useState('');

  async function load() {
    try {
      const data = await api.getSessions();
      setSessions(data);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, []);

  async function revoke(id: string) {
    setRevoking(id);
    try {
      await api.revokeSession(id);
      setSessions(s => s.filter(x => x.id !== id));
    } catch (err: any) {
      setError(err.message);
    } finally {
      setRevoking(null);
    }
  }

  async function logoutAll() {
    setLoggingOutAll(true);
    try {
      await api.logoutAll();
      setSessions([]);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoggingOutAll(false);
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-16" style={{ color: '#6b7280' }}>
        <span style={{ fontSize: 13 }}>Loading sessions…</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Active sessions</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>
            {sessions.length} session{sessions.length !== 1 ? 's' : ''} active
          </p>
        </div>
        {sessions.filter(s => !s.current).length > 0 && (
          <Button variant="danger" size="sm" loading={loggingOutAll} onClick={logoutAll}>
            Revoke all others
          </Button>
        )}
      </div>

      {error && (
        <div
          className="rounded-lg px-3 py-2.5 text-sm"
          style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
        >
          {error}
        </div>
      )}

      {/* Sessions list */}
      <div className="flex flex-col gap-2">
        {sessions.length === 0 && (
          <div className="rounded-xl py-10 text-center" style={{ background: '#0f0f0f', border: '1px solid #1e1e1e' }}>
            <p style={{ color: '#6b7280', fontSize: 13 }}>No active sessions</p>
          </div>
        )}
        {sessions.map(session => (
          <div
            key={session.id}
            className="rounded-xl p-4 flex items-start gap-3"
            style={{
              background: session.current ? '#22c55e08' : '#0f0f0f',
              border: session.current ? '1px solid #22c55e22' : '1px solid #1e1e1e',
            }}
          >
            <SessionIcon current={session.current} />

            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <span className="text-sm font-medium" style={{ color: '#efefef' }}>
                  {session.device_name || parseUA(session.user_agent)}
                </span>
                {session.current && (
                  <span
                    className="text-xs rounded-full px-2 py-0.5 font-medium"
                    style={{ background: '#22c55e1a', color: '#22c55e', border: '1px solid #22c55e33' }}
                  >
                    Current
                  </span>
                )}
              </div>

              <div className="flex flex-col gap-0.5 mt-1.5">
                {session.ip_address && (
                  <span
                    className="text-xs"
                    style={{ color: '#6b7280', fontFamily: 'JetBrains Mono, monospace' }}
                  >
                    {session.ip_address}
                  </span>
                )}
                <span className="text-xs" style={{ color: '#3f3f46' }}>
                  Last active {formatDate(session.last_used_at)}
                </span>
                <span className="text-xs" style={{ color: '#3f3f46' }}>
                  Expires {formatDate(session.expires_at)}
                </span>
              </div>
            </div>

            {!session.current && (
              <Button
                variant="danger"
                size="sm"
                loading={revoking === session.id}
                onClick={() => revoke(session.id)}
              >
                Revoke
              </Button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
