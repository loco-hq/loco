import { Navigate } from 'react-router-dom';
import { useSchema } from './SchemaContext.jsx';

export default function Home() {
  const { collections } = useSchema();

  if (!collections || collections.length === 0) return null;

  // Redirect to the first collection
  const first = collections[0];
  return <Navigate to={`/collection/${first.user}/${first.project}/${first.name}`} replace />;
}
