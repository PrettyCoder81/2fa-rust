import React, { useState } from 'react';
import AuthLayout from '../components/AuthLayout';
import Input from '../components/Input';
import Button from '../components/Button';
import { api } from '../api';

interface Props {
  challengeToken: string;
  onSuccess: () => void;
  onBack: () => void;
}

export default function TwoFactorPage({ challengeToken, onSuccess, onBack }: Props) {
  const [mode, setMode] = useState<'totp' | 'recovery'>('totp');
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      if (mode === 'totp') {
        await api.verify2fa({ challenge_token: challengeToken, code });
      } else {
        await api.verifyRecovery({ challenge_token: challengeToken, code });
      }
      onSuccess();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <AuthLayout
      title="Two-factor verification"
      subtitle={
        mode === 'totp'
          ? 'Enter the 6-digit code from your authenticator app.'
          : 'Enter one of your recovery codes.'
      }
    >
      <form onSubmit={handleSubmit} className="flex flex-col gap-4">
        <Input
          label={mode === 'totp' ? 'Authenticator code' : 'Recovery code'}
          type="text"
          value={code}
          onChange={e => setCode(e.target.value)}
          placeholder={mode === 'totp' ? '000000' : 'XXXX-XXXX-XXXX'}
          autoComplete="one-time-code"
          required
          style={{ fontFamily: 'JetBrains Mono, monospace', letterSpacing: '0.1em', fontSize: '16px' }}
        />

        {error && (
          <div
            className="rounded-lg px-3 py-2.5 text-sm"
            style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
          >
            {error}
          </div>
        )}

        <Button type="submit" loading={loading} style={{ width: '100%' }}>
          Verify
        </Button>

        <div className="flex justify-between items-center">
          <button
            type="button"
            onClick={onBack}
            style={{ color: '#6b7280', background: 'none', border: 'none', cursor: 'pointer', fontSize: '13px' }}
          >
            ← Back to login
          </button>
          <button
            type="button"
            onClick={() => { setMode(mode === 'totp' ? 'recovery' : 'totp'); setCode(''); setError(''); }}
            style={{ color: '#22c55e', background: 'none', border: 'none', cursor: 'pointer', fontSize: '13px' }}
          >
            {mode === 'totp' ? 'Use recovery code' : 'Use authenticator'}
          </button>
        </div>
      </form>
    </AuthLayout>
  );
}
