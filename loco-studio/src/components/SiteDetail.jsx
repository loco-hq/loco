import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getSite, deleteSite, listCollections } from '../api.js';

export default function SiteDetail() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: site, isLoading, error } = useQuery({
    queryKey: ['site', user, project, name],
    queryFn: () => getSite(user, project, name),
  });

  const { data: collections = [] } = useQuery({
    queryKey: ['collections', user, project, site?.version],
    queryFn: () => listCollections(user, project, site.version),
    enabled: !!site?.version,
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

  // Group flat collection list by their owning project — collections inherited
  // from dependency projects appear under their own header.
  const groups = collections.reduce((acc, col) => {
    const ns = col.project || `${user}/${project}`;
    (acc[ns] ||= []).push(col);
    return acc;
  }, {});

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

      <section>
        <div className="section-heading">
          <h3>Collections <span className="count">({collections.length})</span></h3>
        </div>
        {collections.length === 0 && (
          <p className="empty-state">No collections found for this site.</p>
        )}
        {Object.entries(groups).map(([ns, cols]) => (
          <div key={ns} className="namespace-section">
            <h4 className="ns-header">
              <span className="ns-name">{ns}</span>
              <span className="count">({cols.length})</span>
            </h4>
            <div className="list">
              {cols.map((col) => (
                <div key={`${col.project}/${col.name}`} className="list-row">
                  <div className="list-row-main">
                    <span className="list-row-name">{col.name}</span>
                    {col.label && <span className="list-row-label">{col.label}</span>}
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))}
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
