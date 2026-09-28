import React, { useState } from 'react';
import SessionsTab from './SessionsTab';
import TwoFactorTab from './TwoFactorTab';
import PasswordTab from './PasswordTab';
import { api } from '../../api';

type Tab = 'sessions' | '2fa' | 'password';

interface Props {
  initialHas2fa: boolean;
  onLogout: () => void;
}

const TABS: { id: Tab; label: string; icon: React.ReactNode }[] = [
  {
    id: 'sessions',
    label: 'Sessions',
    icon: (
      <svg width="15" height="15" viewBox="0 0 15 15" fill="none">
        <rect x="1.5" y="2.5" width="12" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
        <path d="M4.5 13.5H10.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
        <path d="M7.5 10.5V13.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    id: '2fa',
    label: '2FA',
    icon: (
      <svg width="15" height="15" viewBox="0 0 15 15" fill="none">
        <path d="M7.5 1L13 3.2V7C13 10.3 10.5 13.3 7.5 14.3C4.5 13.3 2 10.3 2 7V3.2L7.5 1Z" stroke="currentColor" strokeWidth="1.2" />
      </svg>
    ),
  },
  {
    id: 'password',
    label: 'Password',
    icon: (
      <svg width="15" height="15" viewBox="0 0 15 15" fill="none">
        <rect x="3" y="7" width="9" height="7" rx="1" stroke="currentColor" strokeWidth="1.2" />
        <path d="M5 7V5a2.5 2.5 0 015 0v2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
        <circle cx="7.5" cy="10.5" r="1" fill="currentColor" />
      </svg>
    ),
  },
];

export default function Dashboard({ initialHas2fa, onLogout }: Props) {
  const [tab, setTab] = useState<Tab>('sessions');
  const [has2fa, setHas2fa] = useState(initialHas2fa);
  const [loggingOut, setLoggingOut] = useState(false);

  async function handleLogout() {
    setLoggingOut(true);
    try {
      await api.logout();
    } catch {}
    onLogout();
  }

  return (
    <div className="min-h-screen flex flex-col" style={{ background: '#080808' }}>
      {/* Subtle grid */}
      <div
        className="fixed inset-0 pointer-events-none"
        style={{
          backgroundImage: `
            linear-gradient(rgba(255,255,255,0.012) 1px, transparent 1px),
            linear-gradient(90deg, rgba(255,255,255,0.012) 1px, transparent 1px)
          `,
          backgroundSize: '40px 40px',
        }}
      />

      {/* Topbar */}
      <header
        className="relative flex items-center justify-between px-6 py-3.5"
        style={{
          background: '#0a0a0a',
          borderBottom: '1px solid #1a1a1a',
          backdropFilter: 'blur(12px)',
          position: 'sticky',
          top: 0,
          zIndex: 10,
        }}
      >
        <div className="flex items-center gap-2">
          <div
            className="w-6 h-6 rounded flex items-center justify-center"
            style={{ background: '#22c55e', boxShadow: '0 0 12px #22c55e55' }}
          >
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none">
              <path d="M7 1L13 4V7C13 10.31 10.31 13 7 13C3.69 13 1 10.31 1 7V4L7 1Z" fill="#080808" />
              <path d="M4.5 7L6.5 9L9.5 5.5" stroke="#22c55e" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
          <span className="text-sm font-semibold tracking-wide" style={{ color: '#efefef' }}>VAULT</span>
        </div>

        <button
          onClick={handleLogout}
          disabled={loggingOut}
          className="flex items-center gap-1.5 text-xs rounded-lg px-3 py-1.5 transition-colors"
          style={{
            background: 'none',
            border: '1px solid #1e1e1e',
            color: loggingOut ? '#3f3f46' : '#6b7280',
            cursor: loggingOut ? 'not-allowed' : 'pointer',
            fontFamily: 'Inter, sans-serif',
          }}
          onMouseEnter={e => { if (!loggingOut) { e.currentTarget.style.color = '#efefef'; e.currentTarget.style.borderColor = '#2a2a2a'; } }}
          onMouseLeave={e => { e.currentTarget.style.color = '#6b7280'; e.currentTarget.style.borderColor = '#1e1e1e'; }}
        >
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
            <path d="M5 2H2a1 1 0 00-1 1v7a1 1 0 001 1h3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <path d="M9 9l3-2.5L9 4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
            <path d="M12 6.5H5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
          {loggingOut ? 'Signing out…' : 'Sign out'}
        </button>
      </header>

      {/* Main */}
      <main className="relative flex-1 max-w-2xl w-full mx-auto px-4 py-8">
        {/* Tab nav */}
        <nav
          className="flex gap-1 rounded-xl p-1 mb-6"
          style={{ background: '#0f0f0f', border: '1px solid #1e1e1e' }}
        >
          {TABS.map(t => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className="flex items-center gap-1.5 flex-1 justify-center rounded-lg py-2 text-xs font-medium transition-all duration-150"
              style={{
                background: tab === t.id ? '#1a1a1a' : 'transparent',
                border: tab === t.id ? '1px solid #2a2a2a' : '1px solid transparent',
                color: tab === t.id ? '#efefef' : '#6b7280',
                fontFamily: 'Inter, sans-serif',
                cursor: 'pointer',
              }}
            >
              <span style={{ color: tab === t.id ? '#22c55e' : 'inherit' }}>{t.icon}</span>
              {t.label}
            </button>
          ))}
        </nav>

        {/* Tab content */}
        <div
          className="rounded-xl p-5"
          style={{ background: '#0f0f0f', border: '1px solid #1e1e1e' }}
        >
          {tab === 'sessions' && <SessionsTab />}
          {tab === '2fa' && <TwoFactorTab has2fa={has2fa} onToggle2fa={setHas2fa} />}
          {tab === 'password' && <PasswordTab />}
        </div>
      </main>
    </div>
  );
}
