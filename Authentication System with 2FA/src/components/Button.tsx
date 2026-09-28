import React from 'react';

interface Props extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'ghost' | 'danger';
  loading?: boolean;
  size?: 'sm' | 'md';
}

export default function Button({
  variant = 'primary',
  loading = false,
  size = 'md',
  children,
  disabled,
  style,
  ...props
}: Props) {
  const base: React.CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: '6px',
    fontFamily: 'Inter, sans-serif',
    fontWeight: 500,
    borderRadius: '8px',
    border: 'none',
    cursor: disabled || loading ? 'not-allowed' : 'pointer',
    opacity: disabled || loading ? 0.5 : 1,
    transition: 'all 0.15s ease',
    fontSize: size === 'sm' ? '12px' : '14px',
    padding: size === 'sm' ? '6px 12px' : '10px 16px',
    whiteSpace: 'nowrap' as const,
  };

  const variants: Record<string, React.CSSProperties> = {
    primary: {
      background: '#22c55e',
      color: '#080808',
      boxShadow: '0 0 16px #22c55e33',
    },
    ghost: {
      background: '#161616',
      color: '#efefef',
      border: '1px solid #1e1e1e',
    },
    danger: {
      background: '#ef44441a',
      color: '#ef4444',
      border: '1px solid #ef444433',
    },
  };

  return (
    <button
      disabled={disabled || loading}
      style={{ ...base, ...variants[variant], ...style }}
      onMouseEnter={e => {
        if (!disabled && !loading) {
          if (variant === 'primary') e.currentTarget.style.background = '#16a34a';
          if (variant === 'ghost') e.currentTarget.style.background = '#1e1e1e';
          if (variant === 'danger') e.currentTarget.style.background = '#ef444426';
        }
      }}
      onMouseLeave={e => {
        if (!disabled && !loading) {
          if (variant === 'primary') e.currentTarget.style.background = '#22c55e';
          if (variant === 'ghost') e.currentTarget.style.background = '#161616';
          if (variant === 'danger') e.currentTarget.style.background = '#ef44441a';
        }
      }}
      {...props}
    >
      {loading && (
        <span
          style={{
            width: 14,
            height: 14,
            border: '2px solid currentColor',
            borderTopColor: 'transparent',
            borderRadius: '50%',
            animation: 'spin 0.7s linear infinite',
            display: 'inline-block',
          }}
        />
      )}
      {children}
    </button>
  );
}
