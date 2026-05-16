import { Fragment } from 'react';
import { Outlet, Link, useLocation, useNavigate } from 'react-router-dom';
import { useMe, useLogout } from '../auth.js';
import logoUrl from '../assets/logo.svg';

function Breadcrumb() {
  const { pathname } = useLocation();
  const parts = pathname.split('/').filter(Boolean);
  if (parts[0] !== 'projects' || parts.length < 3) return null;
  const [, user, project, ...rest] = parts;

  // Collapse `versions/<v>` into a single crumb linking to the version home.
  // `settings` is intentionally hidden — the project crumb itself links there.
  const crumbs = [];
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === 'versions' && i + 1 < rest.length) {
      const v = rest[i + 1];
      crumbs.push({
        text: v,
        section: false,
        href: `/projects/${user}/${project}/versions/${v}`,
      });
      i++;
    } else if (rest[i] === 'settings') {
      continue;
    } else {
      const isSection = ['sites', 'datasets', 'collections', 'fields'].includes(rest[i]);
      crumbs.push({ text: rest[i], section: isSection });
    }
  }

  const projectIsTerminal = crumbs.length === 0;
  const settingsPath = `/projects/${user}/${project}/settings`;

  return (
    <nav className="breadcrumb" aria-label="Breadcrumb">
      <Link to="/">{user}</Link>
      <span className="sep">/</span>
      {projectIsTerminal ? (
        <strong>{project}</strong>
      ) : (
        <Link to={settingsPath}>{project}</Link>
      )}
      {crumbs.map((c, i) => {
        const last = i === crumbs.length - 1;
        return (
          <Fragment key={i}>
            <span className="sep">/</span>
            {last ? (
              <strong>{c.text}</strong>
            ) : c.href ? (
              <Link to={c.href}>{c.text}</Link>
            ) : (
              <span className={c.section ? 'crumb-muted' : undefined}>{c.text}</span>
            )}
          </Fragment>
        );
      })}
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
            <img src={logoUrl} alt="Loco" className="logo-mark" />
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
