import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useSyncExternalStore } from 'react';
import { Link, Outlet } from 'react-router-dom';
import { loco } from '../data.js';
import Login from './Login.jsx';

// Pick state is private: nothing renders until a picker with editor or
// developer access on brocksbricks/orders is signed in.
export default function App() {
  const qc = useQueryClient();
  const loggedIn = useSyncExternalStore(loco.onSessionChange, loco.isLoggedIn);

  // The next person must not see this one's cached records.
  useEffect(
    () =>
      loco.onSessionChange((isIn) => {
        if (!isIn) qc.clear();
      }),
    [qc],
  );

  const me = useQuery({ queryKey: ['me'], queryFn: loco.me, enabled: loggedIn });

  return (
    <>
      <header className="topbar">
        <Link to="/" className="brand">
          <span className="stud" aria-hidden="true" />
          Brock&rsquo;s Bricks <span className="muted">pull</span>
        </Link>
        {loggedIn && (
          <div className="who">
            <span className="muted">{me.data?.username}</span>
            <button className="link" onClick={() => loco.logout().catch(() => {})}>
              Sign out
            </button>
          </div>
        )}
      </header>
      <main>{loggedIn ? <Outlet /> : <Login />}</main>
    </>
  );
}
