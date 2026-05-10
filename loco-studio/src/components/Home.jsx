import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { listProjects } from '../api.js';

export default function Home() {
  const { data: projects, isLoading, error } = useQuery({
    queryKey: ['projects'],
    queryFn: listProjects,
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <section>
      <div className="section-heading">
        <h2>Projects <span className="count">({projects.length})</span></h2>
        <div className="section-heading-actions">
          <Link to="/projects/new" className="btn btn-primary">New project</Link>
        </div>
      </div>
      {projects.length === 0 ? (
        <p className="empty-state">No projects yet.</p>
      ) : (
        <div className="list">
          {projects.map(([id, fields]) => {
            const [u, p] = fields.project.split('/');
            return (
              <Link key={id} to={`/projects/${u}/${p}`} className="list-row">
                <div className="list-row-main">
                  <span className="list-row-name">{fields.project}</span>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
                  {fields.description && <span className="list-row-desc">{fields.description}</span>}
                </div>
              </Link>
            );
          })}
        </div>
      )}
    </section>
  );
}
