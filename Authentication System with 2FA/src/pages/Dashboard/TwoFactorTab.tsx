import React, { useState } from 'react';
import { api } from '../../api';
import Button from '../../components/Button';
import Input from '../../components/Input';

type Step =
  | 'idle'
  | 'setup-qr'
  | 'setup-confirm'
  | 'setup-done'
  | 'disable'
  | 'regen-codes'
  | 'regen-done';

interface Props {
  has2fa: boolean;
  onToggle2fa: (enabled: boolean) => void;
}

function CodeGrid({ codes }: { codes: string[] }) {
  const [copied, setCopied] = useState(false);

  async function copyAll() {
    const text = codes.join('\n');

    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(text);
      } else {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.opacity = '0';
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        document.execCommand('copy');
        document.body.removeChild(textArea);
      }

      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div
      className="rounded-xl p-4"
      style={{ background: '#0a0a0a', border: '1px solid #1e1e1e' }}
    >
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-medium uppercase tracking-wide" style={{ color: '#6b7280', letterSpacing: '0.06em' }}>
          Recovery codes
        </span>
        <button
          type="button"
          onClick={copyAll}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            color: copied ? '#22c55e' : '#6b7280', fontSize: 12, display: 'flex', alignItems: 'center', gap: 4,
          }}
        >
          {copied ? '✓ Copied' : 'Copy all'}
        </button>
      </div>
      <div className="grid grid-cols-2 gap-1.5">
        {codes.map((code, i) => (
          <span
            key={i}
            className="text-xs rounded-lg px-2.5 py-2 text-center"
            style={{
              fontFamily: 'JetBrains Mono, monospace',
              background: '#111111',
              border: '1px solid #222222',
              color: '#efefef',
              letterSpacing: '0.05em',
            }}
          >
            {code}
          </span>
        ))}
      </div>
      <p className="text-xs mt-3" style={{ color: '#6b7280' }}>
        Store these in a safe place. Each code can only be used once.
      </p>
    </div>
  );
}

export default function TwoFactorTab({ has2fa, onToggle2fa }: Props) {
  const [step, setStep] = useState<Step>('idle');
  const [setupData, setSetupData] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [confirmCode, setConfirmCode] = useState('');
  const [disablePassword, setDisablePassword] = useState('');
  const [disableCode, setDisableCode] = useState('');
  const [regenPassword, setRegenPassword] = useState('');
  const [regenCode, setRegenCode] = useState('');
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  function reset() {
    setStep('idle');
    setError('');
    setConfirmCode('');
    setDisablePassword('');
    setDisableCode('');
    setRegenPassword('');
    setRegenCode('');
    setSetupData(null);
    setRecoveryCodes([]);
  }

  async function startSetup() {
    setError('');
    setLoading(true);
    try {
      const data = await api.setup2fa();
      setSetupData(data);
      setStep('setup-qr');
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  async function confirmSetup() {
    setError('');
    setLoading(true);
    try {
      const res = await api.confirm2fa({ code: confirmCode });
      setRecoveryCodes(res.recovery_codes);
      setStep('setup-done');
      onToggle2fa(true);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  async function disable2fa() {
    setError('');
    setLoading(true);
    try {
      await api.disable2fa({ password: disablePassword, code: disableCode });
      onToggle2fa(false);
      reset();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  async function regenCodes() {
    setError('');
    setLoading(true);
    try {
      const res = await api.regenerateRecoveryCodes({ password: regenPassword, code: regenCode });
      setRecoveryCodes(res.recovery_codes);
      setStep('regen-done');
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  const qrUrl = setupData
    ? `https://api.qrserver.com/v1/create-qr-code/?size=180x180&bgcolor=0f0f0f&color=22c55e&data=${encodeURIComponent(setupData.otpauth_url)}`
    : null;

  const ErrorBox = () => error ? (
    <div
      className="rounded-lg px-3 py-2.5 text-sm"
      style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
    >
      {error}
    </div>
  ) : null;

  // --- Setup QR step ---
  if (step === 'setup-qr') {
    return (
      <div className="flex flex-col gap-4">
        <div>
          <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Set up authenticator</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>Scan this QR code with your authenticator app.</p>
        </div>
        <div
          className="rounded-xl p-5 flex flex-col items-center gap-4"
          style={{ background: '#0f0f0f', border: '1px solid #1e1e1e' }}
        >
          {qrUrl && (
            <div className="rounded-lg overflow-hidden" style={{ border: '1px solid #222', background: '#0f0f0f' }}>
              <img src={qrUrl} alt="2FA QR code" width={180} height={180} />
            </div>
          )}
          <div className="w-full">
            <p className="text-xs mb-1" style={{ color: '#6b7280' }}>Or enter manually:</p>
            <div
              className="rounded-lg px-3 py-2 text-center"
              style={{
                fontFamily: 'JetBrains Mono, monospace',
                fontSize: 12,
                letterSpacing: '0.1em',
                background: '#0a0a0a',
                border: '1px solid #1e1e1e',
                color: '#22c55e',
                wordBreak: 'break-all',
              }}
            >
              {setupData?.secret}
            </div>
          </div>
        </div>
        <div className="flex gap-2">
          <Button variant="ghost" onClick={reset} style={{ flex: 1 }}>Cancel</Button>
          <Button onClick={() => { setStep('setup-confirm'); setError(''); }} style={{ flex: 1 }}>
            Next →
          </Button>
        </div>
      </div>
    );
  }

  // --- Confirm code step ---
  if (step === 'setup-confirm') {
    return (
      <div className="flex flex-col gap-4">
        <div>
          <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Verify setup</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>Enter the code from your app to confirm setup.</p>
        </div>
        <Input
          label="Authenticator code"
          type="text"
          value={confirmCode}
          onChange={e => setConfirmCode(e.target.value)}
          placeholder="000000"
          style={{ fontFamily: 'JetBrains Mono, monospace', letterSpacing: '0.15em', fontSize: 18 }}
          autoComplete="one-time-code"
        />
        <ErrorBox />
        <div className="flex gap-2">
          <Button variant="ghost" onClick={() => { setStep('setup-qr'); setError(''); }} style={{ flex: 1 }}>Back</Button>
          <Button loading={loading} onClick={confirmSetup} style={{ flex: 1 }}>Confirm</Button>
        </div>
      </div>
    );
  }

  // --- Setup done step ---
  if (step === 'setup-done') {
    return (
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <div
            className="w-7 h-7 rounded-full flex items-center justify-center"
            style={{ background: '#22c55e1a', border: '1px solid #22c55e33' }}
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M2.5 7L5.5 10L11.5 4" stroke="#22c55e" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
          <div>
            <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>2FA enabled</h2>
            <p className="text-xs" style={{ color: '#6b7280' }}>Save your recovery codes below.</p>
          </div>
        </div>
        <CodeGrid codes={recoveryCodes} />
        <Button onClick={reset} style={{ width: '100%' }}>Done</Button>
      </div>
    );
  }

  // --- Disable 2FA ---
  if (step === 'disable') {
    return (
      <div className="flex flex-col gap-4">
        <div>
          <h2 className="font-semibold" style={{ color: '#ef4444', fontSize: 15 }}>Disable 2FA</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>
            This will remove two-factor authentication from your account.
          </p>
        </div>
        <Input
          label="Current password"
          type="password"
          value={disablePassword}
          onChange={e => setDisablePassword(e.target.value)}
          placeholder="••••••••"
        />
        <Input
          label="Authenticator code"
          type="text"
          value={disableCode}
          onChange={e => setDisableCode(e.target.value)}
          placeholder="000000"
          style={{ fontFamily: 'JetBrains Mono, monospace', letterSpacing: '0.15em' }}
        />
        <ErrorBox />
        <div className="flex gap-2">
          <Button variant="ghost" onClick={reset} style={{ flex: 1 }}>Cancel</Button>
          <Button variant="danger" loading={loading} onClick={disable2fa} style={{ flex: 1 }}>Disable 2FA</Button>
        </div>
      </div>
    );
  }

  // --- Regenerate codes ---
  if (step === 'regen-codes') {
    return (
      <div className="flex flex-col gap-4">
        <div>
          <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Regenerate recovery codes</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>Old codes will be invalidated.</p>
        </div>
        <Input
          label="Current password"
          type="password"
          value={regenPassword}
          onChange={e => setRegenPassword(e.target.value)}
          placeholder="••••••••"
        />
        <Input
          label="Authenticator code"
          type="text"
          value={regenCode}
          onChange={e => setRegenCode(e.target.value)}
          placeholder="000000"
          style={{ fontFamily: 'JetBrains Mono, monospace', letterSpacing: '0.15em' }}
        />
        <ErrorBox />
        <div className="flex gap-2">
          <Button variant="ghost" onClick={reset} style={{ flex: 1 }}>Cancel</Button>
          <Button loading={loading} onClick={regenCodes} style={{ flex: 1 }}>Regenerate</Button>
        </div>
      </div>
    );
  }

  // --- Regen done ---
  if (step === 'regen-done') {
    return (
      <div className="flex flex-col gap-4">
        <div>
          <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>New recovery codes</h2>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>Previous codes are no longer valid.</p>
        </div>
        <CodeGrid codes={recoveryCodes} />
        <Button onClick={reset} style={{ width: '100%' }}>Done</Button>
      </div>
    );
  }

  // --- Idle / Main view ---
  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Two-factor authentication</h2>
        <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>
          Add an extra layer of security to your account.
        </p>
      </div>

      <div
        className="rounded-xl p-4 flex items-start gap-3"
        style={{
          background: has2fa ? '#22c55e08' : '#0f0f0f',
          border: has2fa ? '1px solid #22c55e22' : '1px solid #1e1e1e',
        }}
      >
        <div
          className="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0"
          style={{
            background: has2fa ? '#22c55e1a' : '#161616',
            border: has2fa ? '1px solid #22c55e33' : '1px solid #1e1e1e',
          }}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path
              d="M8 1.5L13 3.5V7C13 10.5 10.5 13.5 8 14.5C5.5 13.5 3 10.5 3 7V3.5L8 1.5Z"
              stroke={has2fa ? '#22c55e' : '#6b7280'}
              strokeWidth="1.3"
            />
            {has2fa && (
              <path d="M5.5 8L7.5 10L10.5 6" stroke="#22c55e" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
            )}
          </svg>
        </div>
        <div>
          <p className="text-sm font-medium" style={{ color: '#efefef' }}>
            {has2fa ? 'Authenticator app' : 'Not enabled'}
          </p>
          <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>
            {has2fa ? '2FA is active on your account.' : 'Protect your account with a TOTP app.'}
          </p>
        </div>
      </div>

      {error && (
        <div
          className="rounded-lg px-3 py-2.5 text-sm"
          style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
        >
          {error}
        </div>
      )}

      {!has2fa ? (
        <Button loading={loading} onClick={startSetup} style={{ width: '100%' }}>
          Enable two-factor authentication
        </Button>
      ) : (
        <div className="flex flex-col gap-2">
          <Button variant="ghost" onClick={() => setStep('regen-codes')} style={{ width: '100%' }}>
            Regenerate recovery codes
          </Button>
          <Button variant="danger" onClick={() => setStep('disable')} style={{ width: '100%' }}>
            Disable 2FA
          </Button>
        </div>
      )}
    </div>
  );
}
