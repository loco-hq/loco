import { useState } from 'react';
import { loco } from '../data.js';

// Signing in flips the session App watches; no callback needed.
export default function Login() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  async function submit(e) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await loco.login(username, password);
    } catch (err) {
      setError(err.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="card login" onSubmit={submit}>
      <h1>Sign in</h1>
      <label>
        Username
        <input
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          autoComplete="username"
          autoFocus
        />
      </label>
      <label>
        Password
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="current-password"
        />
      </label>
      {error && <p className="error">{error}</p>}
      <button className="primary" disabled={busy || !username}>
        {busy ? 'Signing in…' : 'Sign in'}
      </button>
    </form>
  );
}
