import { Outlet, Link, useNavigate } from 'react-router-dom';
import { useMe, useLogout } from '../auth.js';

export default function Layout() {
  const navigate = useNavigate();
  const { data: user } = useMe();
  const logout = useLogout();

  const handleLogout = () => {
    logout.mutate(undefined, { onSuccess: () => navigate('/login') });
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
