import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';

export default function Login() {
  const { loggedIn, login } = useAuth();
  const navigate = useNavigate();
  const [error, setError] = useState('');

  if (loggedIn) {
    navigate('/');
    return null;
  }

  const handleSubmit = async (e) => {
    e.preventDefault();
    const username = e.target.elements.username.value;
    setError('');
    try {
      await login(username);
      navigate('/');
    } catch (err) {
      setError(err.message);
    }
  };

  return (
    <div className="login-page">
      <div className="login-card">
        <h1>Loco Studio</h1>
        <p className="subtitle">Sign in to manage your schemas</p>
        <form onSubmit={handleSubmit}>
          <input name="username" type="text" placeholder="Username" required />
          <button type="submit">Sign in</button>
        </form>
        {error && <p className="error">{error}</p>}
      </div>
    </div>
  );
}
