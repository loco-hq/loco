import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Link, Outlet } from 'react-router-dom';
import { getMe, isLoggedIn, logout } from '../api.js';
import Login from './Login.jsx';

// Inventory is private: nothing renders until someone with editor or
// developer access on brickos/inventory is signed in.
export default function App() {
  const qc = useQueryClient();
  const [loggedIn, setLoggedIn] = useState(isLoggedIn);

  useEffect(() => {
    const onLogout = () => {
      setLoggedIn(false);
      qc.clear();
    };
    window.addEventListener('brickos:logout', onLogout);
    return () => window.removeEventListener('brickos:logout', onLogout);
  }, [qc]);

  const me = useQuery({ queryKey: ['me'], queryFn: getMe, enabled: loggedIn });

  async function signOut() {
    await logout().catch(() => {});
    window.dispatchEvent(new Event('brickos:logout'));
  }

  return (
    <>
      <header className="topbar">
        <Link to="/" className="brand">
          <span className="stud" aria-hidden="true" />
          BrickOS <span className="muted">inventory</span>
        </Link>
        {loggedIn && (
          <div className="who">
            <span className="muted">{me.data?.username}</span>
            <button className="link" onClick={signOut}>
              Sign out
            </button>
          </div>
        )}
      </header>
      <main>{loggedIn ? <Outlet /> : <Login onLogin={() => setLoggedIn(true)} />}</main>
    </>
  );
}
