import { useState } from 'react';
import { Navigate, useNavigate } from 'react-router-dom';
import { TextField } from 'loco-ui';
import { isLoggedIn, useLogin } from '../auth.js';
import logoUrl from '../assets/logo.svg';

export default function Login() {
  const navigate = useNavigate();
  const login = useLogin();
  const [username, setUsername] = useState('');

  if (isLoggedIn()) return <Navigate to="/" replace />;

  const handleSubmit = (e) => {
    e.preventDefault();
    login.mutate(username, { onSuccess: () => navigate('/') });
  };

  return (
    <div className="login-page">
      <div className="login-card">
        <img src={logoUrl} alt="Loco" className="login-logo" />
        <p className="subtitle">Sign in to manage your schemas</p>
        <form onSubmit={handleSubmit}>
          <TextField
            placeholder="Username"
            required
            value={username}
            onChange={setUsername}
          />
          <button type="submit" disabled={login.isPending}>Sign in</button>
        </form>
        {login.error && <p className="error">{login.error.message}</p>}
      </div>
    </div>
  );
}
