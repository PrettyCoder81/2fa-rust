import React, { useState } from 'react';
import AuthLayout from '../components/AuthLayout';
import Input from '../components/Input';
import Button from '../components/Button';
import { api } from '../api';

interface Props {
  onSuccess: () => void;
  onLogin: () => void;
}

export default function RegisterPage({ onSuccess, onLogin }: Props) {
  const [username, setUsername] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      await api.register({ username, email, password });
      onSuccess();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <AuthLayout title="Create account" subtitle="Join to manage your secure sessions.">
      <form onSubmit={handleSubmit} className="flex flex-col gap-4">
        <Input
          label="Username"
          type="text"
          value={username}
          onChange={e => setUsername(e.target.value)}
          placeholder="johndoe"
          autoComplete="username"
          required
        />
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
          autoComplete="new-password"
          required
          minLength={6}
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
          Create account
        </Button>

        <p className="text-sm text-center" style={{ color: '#6b7280' }}>
          Already have an account?{' '}
          <button
            type="button"
            onClick={onLogin}
            style={{ color: '#22c55e', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
          >
            Sign in
          </button>
        </p>
      </form>
    </AuthLayout>
  );
}
