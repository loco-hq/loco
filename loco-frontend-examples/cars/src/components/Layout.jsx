import { Outlet, NavLink } from 'react-router-dom';
import { useSchema } from './SchemaContext.jsx';

export default function Layout() {
  const { collections, error } = useSchema();

  if (error) return <p className="error">Error loading schema: {error}</p>;
  if (!collections) return <p>Loading schema...</p>;

  return (
    <>
      <h1>Loco Cars Demo</h1>
      <nav>
        {collections.map((c) => (
          <NavLink
            key={`${c.namespace}/${c.name}`}
            to={`/collection/${c.user}/${c.project}/${c.name}`}
          >
            {c.label}
          </NavLink>
        ))}
      </nav>
      <Outlet />
    </>
  );
}
