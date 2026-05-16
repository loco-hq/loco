import { useParams, Navigate, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { listVersions } from '../api.js';

export default function ProjectDetail() {
  const { user, project } = useParams();

  const { data: versions = [], isLoading, error } = useQuery({
    queryKey: ['versions', user, project],
    queryFn: () => listVersions(user, project),
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  if (versions.length > 0) {
    const [, fields] = versions[0];
    return <Navigate to={`/projects/${user}/${project}/versions/${fields.version}`} replace />;
  }

  return (
    <section className="detail-header">
      <div className="detail-header-row">
        <h2>{project}</h2>
        <Link to={`/projects/${user}/${project}/settings`} className="btn">Project settings</Link>
      </div>
      <p className="resource-id">{user}/{project}</p>
      <p className="empty-state">
        No versions yet.{' '}
        <Link to={`/projects/${user}/${project}/versions/new`}>Create one</Link> to start
        defining a schema.
      </p>
    </section>
  );
}
