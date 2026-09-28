import React, { useState } from 'react';
import { api } from '../../api';
import Button from '../../components/Button';
import Input from '../../components/Input';

export default function PasswordTab() {
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');
    setSuccess(false);
    if (next !== confirm) {
      setError('New passwords do not match.');
      return;
    }
    if (next.length < 6) {
      setError('Password must be at least 6 characters.');
      return;
    }
    setLoading(true);
    try {
      await api.changePassword({ current_password: current, new_password: next });
      setSuccess(true);
      setCurrent('');
      setNext('');
      setConfirm('');
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="font-semibold" style={{ color: '#efefef', fontSize: 15 }}>Change password</h2>
        <p className="text-xs mt-0.5" style={{ color: '#6b7280' }}>
          Use a strong password at least 8 characters long.
        </p>
      </div>

      <form onSubmit={handleSubmit} className="flex flex-col gap-4">
        <Input
          label="Current password"
          type="password"
          value={current}
          onChange={e => setCurrent(e.target.value)}
          placeholder="••••••••"
          autoComplete="current-password"
          required
        />
        <div
          className="rounded-xl p-4 flex flex-col gap-4"
          style={{ background: '#0f0f0f', border: '1px solid #1e1e1e' }}
        >
          <Input
            label="New password"
            type="password"
            value={next}
            onChange={e => setNext(e.target.value)}
            placeholder="••••••••"
            autoComplete="new-password"
            required
          />
          <Input
            label="Confirm new password"
            type="password"
            value={confirm}
            onChange={e => setConfirm(e.target.value)}
            placeholder="••••••••"
            autoComplete="new-password"
            error={confirm && next && confirm !== next ? 'Passwords do not match' : undefined}
            required
          />
        </div>

        {error && (
          <div
            className="rounded-lg px-3 py-2.5 text-sm"
            style={{ background: '#ef44441a', border: '1px solid #ef444433', color: '#ef4444' }}
          >
            {error}
          </div>
        )}
        {success && (
          <div
            className="rounded-lg px-3 py-2.5 text-sm"
            style={{ background: '#22c55e1a', border: '1px solid #22c55e33', color: '#22c55e' }}
          >
            Password updated successfully.
          </div>
        )}

        <Button type="submit" loading={loading} style={{ width: '100%' }}>
          Update password
        </Button>
      </form>
    </div>
  );
}
