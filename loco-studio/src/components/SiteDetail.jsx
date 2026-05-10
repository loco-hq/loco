import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getSite, deleteSite, updateSite,
  listDatasets, listCollections,
} from '../api.js';

export default function SiteDetail() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const siteKey = ['site', user, project, name];

  const { data: site, isLoading, error } = useQuery({
    queryKey: siteKey,
    queryFn: () => getSite(user, project, name),
  });

  const { data: datasets = [] } = useQuery({
    queryKey: ['datasets', user, project],
    queryFn: () => listDatasets(user, project),
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

  const setDataset = useMutation({
    mutationFn: (dataset) => updateSite(user, project, name, { dataset }),
    onSuccess: () => qc.invalidateQueries({ queryKey: siteKey }),
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
        <h2>{site.label || 'Unnamed Site'}</h2>
        <p className="project-ns">{site.name || ''}</p>
        <p className="site-ns-detail">Project: <code>{user}/{project}</code></p>
        <p className="site-ns-detail">Version: <code>{site.version || ''}</code></p>
        <div className="site-dataset-detail">
          Dataset:{' '}
          <select
            value={site.dataset || ''}
            onChange={(e) => setDataset.mutate(e.target.value)}
          >
            <option value="">None</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
              </option>
            ))}
          </select>
        </div>
        <button className="delete-btn" onClick={() => remove.mutate()}>Delete Site</button>
      </section>

      <section>
        <h3>Collections <span className="count">({collections.length})</span></h3>
        {collections.length === 0 && (
          <p className="empty-state">No collections found for this site.</p>
        )}
        {Object.entries(groups).map(([ns, cols]) => (
          <div key={ns} className="namespace-section">
            <h4 className="ns-header">
              <span className="ns-name">{ns}</span>
              <span className="count">({cols.length})</span>
            </h4>
            <div className="collections-grid">
              {cols.map((col) => (
                <div key={`${col.project}/${col.name}`} className="collection-card">
                  <h4>{col.label || col.name}</h4>
                  <p className="ns">{ns}.{col.name}</p>
                </div>
              ))}
            </div>
          </div>
        ))}
      </section>
    </>
  );
}
