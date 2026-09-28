import React from 'react';

interface Props extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  error?: string;
}

export default function Input({ label, error, id, ...props }: Props) {
  const inputId = id || label.toLowerCase().replace(/\s+/g, '-');
  return (
    <div className="flex flex-col gap-1.5">
      <label
        htmlFor={inputId}
        className="text-xs font-medium tracking-wide uppercase"
        style={{ color: '#6b7280', letterSpacing: '0.06em' }}
      >
        {label}
      </label>
      <input
        id={inputId}
        className="w-full rounded-lg px-3 py-2.5 text-sm outline-none transition-all duration-150"
        style={{
          background: '#0a0a0a',
          border: error ? '1px solid #ef4444' : '1px solid #1e1e1e',
          color: '#efefef',
          fontFamily: 'Inter, sans-serif',
        }}
        onFocus={e => {
          e.currentTarget.style.border = error
            ? '1px solid #ef4444'
            : '1px solid #22c55e';
          e.currentTarget.style.boxShadow = error
            ? '0 0 0 3px #ef44441a'
            : '0 0 0 3px #22c55e1a';
        }}
        onBlur={e => {
          e.currentTarget.style.border = error ? '1px solid #ef4444' : '1px solid #1e1e1e';
          e.currentTarget.style.boxShadow = 'none';
        }}
        {...props}
      />
      {error && <p className="text-xs" style={{ color: '#ef4444' }}>{error}</p>}
    </div>
  );
}
