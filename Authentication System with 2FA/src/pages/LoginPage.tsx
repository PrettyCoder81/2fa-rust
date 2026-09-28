import React, { useState } from 'react';
import AuthLayout from '../components/AuthLayout';
import Input from '../components/Input';
import Button from '../components/Button';
import { api } from '../api';

interface Props {
  onSuccess: (requires2fa: boolean, challengeToken?: string) => void;
  onRegister: () => void;
}

export default function LoginPage({ onSuccess, onRegister }: Props) {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      const res = await api.login({ email, password });
      onSuccess(res.requires_2fa, res.challenge_token);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <AuthLayout title="Sign in" subtitle="Enter your credentials to continue.">
      <form onSubmit={handleSubmit} className="flex flex-col gap-4">
        <Input
          label="Email"
          type="email"
          value={email}
          onChange={e => setEmail(e.target.value)}
          placeholder="you@example.com"
          autoComplete="email"
          required
        />
        <Input
          label="Password"
          type="password"
          value={password}
          onChange={e => setPassword(e.target.value)}
          placeholder="••••••••"
          autoComplete="current-password"
          required
        />

        {error && (
          <div
            className="rounded-lg px-3 py-2.5 text-sm"
            style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
          >
            {error}
          </div>
        )}

        <Button type="submit" loading={loading} style={{ width: '100%', marginTop: '4px' }}>
          Sign in
        </Button>

        <p className="text-sm text-center" style={{ color: '#6b7280' }}>
          No account?{' '}
          <button
            type="button"
            onClick={onRegister}
            className="transition-colors"
            style={{ color: '#22c55e', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
          >
            Register
          </button>
        </p>
      </form>
    </AuthLayout>
  );
}
