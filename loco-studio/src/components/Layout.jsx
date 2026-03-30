import { useEffect } from 'react';
import { Outlet, Link, useNavigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';

export default function Layout() {
  const { user, loggedIn, logout, fetchUser } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    if (!loggedIn) {
      navigate('/login');
      return;
    }
    if (!user) fetchUser();
  }, [loggedIn, user, fetchUser, navigate]);

  if (!loggedIn) return null;

  const handleLogout = async () => {
    await logout();
    navigate('/login');
  };

  return (
    <div className="home">
      <header>
        <div className="header-left">
          <Link to="/" className="logo">Loco Studio</Link>
        </div>
        <div className="header-right">
          {user && <span className="user-info">{user.username}</span>}
          <button id="logout-btn" onClick={handleLogout}>Sign out</button>
        </div>
      </header>
      <main>
        <Outlet />
      </main>
    </div>
  );
}
