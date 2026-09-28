import { useEffect, useState } from 'react';
import LoginPage from './pages/LoginPage';
import RegisterPage from './pages/RegisterPage';
import TwoFactorPage from './pages/TwoFactorPage';
import Dashboard from './pages/Dashboard';
import { api } from './api';

type View = 'login' | 'register' | '2fa' | 'dashboard';

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [challengeToken, setChallengeToken] = useState('');
  const [has2fa, setHas2fa] = useState(false);

  useEffect(() => {
    let cancelled = false;

    api.profile()
      .then(profile => {
        if (!cancelled) {
          setHas2fa(profile.two_factor_enabled);
          setView('dashboard');
        }
      })
      .catch(() => {
        if (!cancelled) setView('login');
      });

    return () => {
      cancelled = true;
    };
  }, []);

  function handleLoginSuccess(requires2fa: boolean, token?: string) {
    setHas2fa(requires2fa);
    if (requires2fa && token) {
      setChallengeToken(token);
      setView('2fa');
      return;
    }

    setChallengeToken('');
    setView('dashboard');
  }

  function handleTwoFactorSuccess() {
    setChallengeToken('');
    setView('dashboard');
  }

  function handleLogout() {
    setChallengeToken('');
    setHas2fa(false);
    setView('login');
  }

  return (
    <>
      <style>{`
        @keyframes spin {
          to { transform: rotate(360deg); }
        }
      `}</style>

      {view === null && <div className="min-h-screen" style={{ background: '#080808' }} />}

      {view === 'login' && (
        <LoginPage
          onSuccess={handleLoginSuccess}
          onRegister={() => setView('register')}
        />
      )}

      {view === 'register' && (
        <RegisterPage
          onSuccess={() => setView('login')}
          onLogin={() => setView('login')}
        />
      )}

      {view === '2fa' && (
        <TwoFactorPage
          challengeToken={challengeToken}
          onSuccess={handleTwoFactorSuccess}
          onBack={() => {
            setChallengeToken('');
            setView('login');
          }}
        />
      )}

      {view === 'dashboard' && (
        <Dashboard
          initialHas2fa={has2fa}
          onLogout={handleLogout}
        />
      )}
    </>
  );
}
