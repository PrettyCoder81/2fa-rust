import React from 'react';

interface Props {
  children: React.ReactNode;
  title: string;
  subtitle?: string;
}

export default function AuthLayout({ children, title, subtitle }: Props) {
  return (
    <div className="min-h-screen flex items-center justify-center p-4" style={{ background: '#080808' }}>
      {/* Subtle grid background */}
      <div
        className="fixed inset-0 pointer-events-none"
        style={{
          backgroundImage: `
            linear-gradient(rgba(255,255,255,0.015) 1px, transparent 1px),
            linear-gradient(90deg, rgba(255,255,255,0.015) 1px, transparent 1px)
          `,
          backgroundSize: '40px 40px',
        }}
      />

      <div className="relative w-full max-w-sm">
        {/* Logo mark */}
        <div className="flex items-center gap-2 mb-8">
          <div
            className="w-7 h-7 rounded flex items-center justify-center"
            style={{ background: '#22c55e', boxShadow: '0 0 16px #22c55e66' }}
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M7 1L13 4V7C13 10.31 10.31 13 7 13C3.69 13 1 10.31 1 7V4L7 1Z" fill="#080808" />
              <path d="M4.5 7L6.5 9L9.5 5.5" stroke="#22c55e" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
          <span className="text-sm font-semibold tracking-wide" style={{ fontFamily: 'Inter', color: '#efefef' }}>
            VAULT
          </span>
        </div>

        {/* Card */}
        <div
          className="rounded-xl p-6"
          style={{
            background: '#0f0f0f',
            border: '1px solid #1e1e1e',
            boxShadow: '0 0 0 1px #0a0a0a, 0 24px 48px -12px rgba(0,0,0,0.8)',
          }}
        >
          <div className="mb-6">
            <h1 className="text-lg font-semibold mb-1" style={{ color: '#efefef' }}>
              {title}
            </h1>
            {subtitle && (
              <p className="text-sm" style={{ color: '#6b7280' }}>
                {subtitle}
              </p>
            )}
          </div>
          {children}
        </div>
      </div>
    </div>
  );
}
