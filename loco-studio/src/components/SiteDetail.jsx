import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getSite, deleteSite, updateSite, listProjects, listDatasets, getSiteCollections } from '../api.js';

export default function SiteDetail() {
  const { '*': siteId } = useParams();
  const navigate = useNavigate();
  const [site, setSite] = useState(null);
  const [projectEntry, setProjectEntry] = useState(null);
  const [allDatasets, setAllDatasets] = useState([]);
  const [schemaNamespaces, setSchemaNamespaces] = useState([]);
  const [error, setError] = useState(null);

  // Derive project config ID from site config ID
  // e.g. "ben/crm/sites/acme" → "ben/crm/project"
  const projectConfigId = siteId.replace(/\/sites\/.*$/, '/project');
  const nsPrefix = siteId.replace(/\/sites\/.*$/, '/');

  const load = useCallback(async () => {
    try {
      const s = await getSite(siteId);
      setSite(s);

      const [allProjects, datasets] = await Promise.all([listProjects(), listDatasets()]);

      const proj = allProjects.find(([id]) => id === projectConfigId);
      if (proj) setProjectEntry(proj);

      setAllDatasets(datasets.filter(([id]) => id.startsWith(nsPrefix + 'datasets/')));

      if (s.site_id) {
        try {
          setSchemaNamespaces(await getSiteCollections(s.site_id));
        } catch { setSchemaNamespaces([]); }
      }
    } catch (err) {
      setError(err.message);
    }
  }, [siteId, projectConfigId, nsPrefix]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteSite(siteId);
    navigate(projectEntry ? `/project/${projectEntry[0]}` : '/');
  };

  const handleDatasetChange = async (e) => {
    const newDataset = e.target.value;
    await updateSite(siteId, { dataset: newDataset });
    load();
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!site) return <p>Loading...</p>;

  const totalCollections = schemaNamespaces.reduce((sum, ns) => sum + ns.collections.length, 0);

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {projectEntry && (
          <> / <Link to={`/project/${projectEntry[0]}`}>{projectEntry[1].name || 'Unnamed'}</Link></>
        )}
        {' / '}<strong>{site.name || site.site_id || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{site.name || 'Unnamed Site'}</h2>
        <p className="project-ns">{site.site_id || ''}</p>
        {site.namespace && <p className="site-ns-detail">Namespace: <code>{site.namespace}</code></p>}
        <div className="site-dataset-detail">
          Dataset:{' '}
          <select value={site.dataset || ''} onChange={handleDatasetChange}>
            <option value="">None</option>
            {allDatasets.map(([id, fields]) => (
              <option key={id} value={fields.dataset_id || ''}>
                {fields.name || fields.dataset_id}
              </option>
            ))}
          </select>
        </div>
        {projectEntry && (
          <p className="site-project-detail">
            Project: <Link to={`/project/${projectEntry[0]}`} className="row-link">
              {projectEntry[1].name || 'Unnamed'}
            </Link>
          </p>
        )}
        <button className="delete-btn" onClick={handleDelete}>Delete Site</button>
      </section>

      <section>
        <h3>Collections <span className="count">({totalCollections})</span></h3>
        {schemaNamespaces.length === 0 && (
          <p className="empty-state">No collections found for this site.</p>
        )}
        {schemaNamespaces.map((ns) => (
          <div key={ns.namespace} className="namespace-section">
            <h4 className="ns-header">
              <span className="ns-name">{ns.namespace}</span>
              <span className="count">({ns.collections.length})</span>
            </h4>
            <div className="collections-grid">
              {ns.collections.map((col) => (
                <div key={col.name} className="collection-card">
                  <h4>{col.fields.label || col.name}</h4>
                  <p className="ns">{ns.namespace}.{col.name}</p>
                  <div className="fields-list">
                    {col.collection_fields.length === 0 && (
                      <span className="no-fields">No fields</span>
                    )}
                    {col.collection_fields.map(([, f]) => (
                      <span key={f.name} className="field-tag">
                        {f.name} <small>{f.type}</small>
                      </span>
                    ))}
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
