import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getSite, deleteSite } from '../api.js';

export default function SiteDetail() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: site, isLoading, error } = useQuery({
    queryKey: ['site', user, project, name],
    queryFn: () => getSite(user, project, name),
  });

  const remove = useMutation({
    mutationFn: () => deleteSite(user, project, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['sites', user, project] });
      navigate(`/projects/${user}/${project}`);
    },
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{site.label || 'Unnamed Site'}</h2>
          <Link to={`/projects/${user}/${project}/sites/${name}/edit`} className="btn">Edit</Link>
        </div>
        <p className="resource-id">{site.name}</p>
        <p className="detail-meta">Project: <code>{user}/{project}</code></p>
        <p className="detail-meta">Version: <code>{site.version || ''}</code></p>
        <p className="detail-meta">Dataset: <code>{site.dataset || 'none'}</code></p>
      </section>

      <section className="danger-zone">
        <h3 className="danger-zone-heading">Danger zone</h3>
        <div className="danger-row">
          <div className="danger-row-info">
            <strong>Delete this site</strong>
            <p>The site config will be permanently removed. The dataset and its records are not affected.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete site
          </button>
        </div>
      </section>
    </>
  );
}
