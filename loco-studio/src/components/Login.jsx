import { Navigate, useNavigate } from 'react-router-dom';
import { isLoggedIn, useLogin } from '../auth.js';
import logoUrl from '../assets/logo.svg';

export default function Login() {
  const navigate = useNavigate();
  const login = useLogin();

  if (isLoggedIn()) return <Navigate to="/" replace />;

  const handleSubmit = (e) => {
    e.preventDefault();
    const username = e.target.elements.username.value;
    login.mutate(username, { onSuccess: () => navigate('/') });
  };

  return (
    <div className="login-page">
      <div className="login-card">
        <img src={logoUrl} alt="Loco" className="login-logo" />
        <p className="subtitle">Sign in to manage your schemas</p>
        <form onSubmit={handleSubmit}>
          <input name="username" type="text" placeholder="Username" required />
          <button type="submit" disabled={login.isPending}>Sign in</button>
        </form>
        {login.error && <p className="error">{login.error.message}</p>}
      </div>
    </div>
  );
}
