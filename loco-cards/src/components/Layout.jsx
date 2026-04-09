import { useEffect } from 'react';
import { Outlet, useNavigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';

export default function Layout() {
  const { loggedIn, user, logout, fetchUser } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    if (!loggedIn) {
      navigate('/login');
    } else if (!user) {
      fetchUser();
    }
  }, [loggedIn, user, navigate, fetchUser]);

  if (!loggedIn) return null;

  const handleLogout = async () => {
    await logout();
    navigate('/login');
  };

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-left">
          <span className="logo-icon">&#9827;</span>
          <span className="logo-text">Loco Cards</span>
        </div>
        <div className="topbar-right">
          {user && <span className="user-name">{user.name}</span>}
          <button className="logout-btn" onClick={handleLogout}>Sign out</button>
        </div>
      </header>
      <main className="content">
        <Outlet />
      </main>
    </div>
  );
}
