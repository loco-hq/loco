import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import {
  getSite, deleteSite, updateSite,
  getProject, listDatasetsForProject, listCollections,
} from '../api.js';

export default function SiteDetail() {
  const { '*': siteId } = useParams();
  const navigate = useNavigate();
  const [site, setSite] = useState(null);
  const [project, setProject] = useState(null);
  const [datasets, setDatasets] = useState([]);
  const [collections, setCollections] = useState([]);
  const [error, setError] = useState(null);

  // "ben/crm/sites/acme" → user="ben", projectName="crm"
  const [user, projectName] = siteId.split('/');
  const projectConfigId = `${user}/${projectName}/project`;

  const load = useCallback(async () => {
    try {
      const s = await getSite(siteId);
      setSite(s);

      const [proj, datasetEntries] = await Promise.all([
        getProject(projectConfigId).catch(() => null),
        listDatasetsForProject(user, projectName),
      ]);
      setProject(proj);
      setDatasets(datasetEntries);

      if (s.version) {
        try {
          setCollections(await listCollections(user, projectName, s.version));
        } catch { setCollections([]); }
      }
    } catch (err) {
      setError(err.message);
    }
  }, [siteId, projectConfigId, user, projectName]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteSite(siteId);
    navigate(`/project/${projectConfigId}`);
  };

  const handleDatasetChange = async (e) => {
    await updateSite(siteId, { dataset: e.target.value });
    load();
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!site) return <p>Loading...</p>;

  // Group flat collection list by their owning project (mimics the prior
  // namespace grouping — collections from dependency projects appear under
  // their own header).
  const collectionsByProject = collections.reduce((acc, col) => {
    const ns = col.project || `${user}/${projectName}`;
    (acc[ns] ||= []).push(col);
    return acc;
  }, {});

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {project && (
          <> / <Link to={`/project/${projectConfigId}`}>{project.label || 'Unnamed'}</Link></>
        )}
        {' / '}<strong>{site.label || site.name || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{site.label || 'Unnamed Site'}</h2>
        <p className="project-ns">{site.name || ''}</p>
        <p className="site-ns-detail">Project: <code>{user}/{projectName}</code></p>
        <p className="site-ns-detail">Version: <code>{site.version || ''}</code></p>
        <div className="site-dataset-detail">
          Dataset:{' '}
          <select value={site.dataset || ''} onChange={handleDatasetChange}>
            <option value="">None</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
              </option>
            ))}
          </select>
        </div>
        <button className="delete-btn" onClick={handleDelete}>Delete Site</button>
      </section>

      <section>
        <h3>Collections <span className="count">({collections.length})</span></h3>
        {collections.length === 0 && (
          <p className="empty-state">No collections found for this site.</p>
        )}
        {Object.entries(collectionsByProject).map(([ns, cols]) => (
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
