import { Outlet, Link, useLocation, useNavigate } from 'react-router-dom';
import { useMe, useLogout } from '../auth.js';

function Breadcrumb() {
  const { pathname } = useLocation();
  const parts = pathname.split('/').filter(Boolean);
  if (parts[0] !== 'projects' || parts.length < 3) return null;
  const [, user, project, section, name] = parts;
  return (
    <nav className="breadcrumb" aria-label="Breadcrumb">
      <Link to={`/projects/${user}/${project}`}>{user}/{project}</Link>
      {section && (
        <>
          <span className="sep">/</span>
          <span className="crumb-muted">{section}</span>
          <span className="sep">/</span>
          <strong>{name}</strong>
        </>
      )}
    </nav>
  );
}

const AVATAR_COLORS = ['#0969da', '#1f883d', '#bf8700', '#cf222e', '#8250df', '#0a3069'];

function Avatar({ name }) {
  const initial = (name || '?').charAt(0).toUpperCase();
  const color = name
    ? AVATAR_COLORS[name.charCodeAt(0) % AVATAR_COLORS.length]
    : '#6e7781';
  return (
    <div className="avatar" style={{ background: color }} title={name || ''}>
      {initial}
    </div>
  );
}

export default function Layout() {
  const navigate = useNavigate();
  const { data: user } = useMe();
  const logout = useLogout();

  const handleLogout = () => {
    logout.mutate(undefined, { onSuccess: () => navigate('/login') });
  };

  return (
    <div className="app">
      <header className="app-header">
        <div className="header-left">
          <Link to="/" className="logo" aria-label="Home">
            <span className="logo-mark">L</span>
          </Link>
          <Breadcrumb />
        </div>
        <div className="header-right">
          <button className="btn-link" onClick={handleLogout}>Sign out</button>
          <Avatar name={user?.username} />
        </div>
      </header>
      <main className="app-main">
        <Outlet />
      </main>
    </div>
  );
}
