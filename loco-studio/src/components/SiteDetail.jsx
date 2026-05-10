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
        <p className="resource-id">{site.name}</p>
        <p className="detail-meta">Project: <code>{user}/{project}</code></p>
        <p className="detail-meta">Version: <code>{site.version || ''}</code></p>
        <div className="detail-dataset">
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
        <button className="delete-btn" onClick={() => remove.mutate()}>Delete site</button>
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
    </>
  );
}
