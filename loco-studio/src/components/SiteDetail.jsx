import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getSite, deleteSite, getProject, listDatasets, getSiteCollections } from '../api.js';

export default function SiteDetail() {
  const { siteId } = useParams();
  const navigate = useNavigate();
  const [site, setSite] = useState(null);
  const [project, setProject] = useState(null);
  const [dataset, setDataset] = useState(null);
  const [schemaNamespaces, setSchemaNamespaces] = useState([]);
  const [error, setError] = useState(null);

  const load = useCallback(async () => {
    try {
      const s = await getSite(siteId);
      setSite(s);
      const sf = s.fields;

      if (sf.project) {
        try { setProject(await getProject(sf.project)); } catch { /* ignore */ }
      }

      if (sf.dataset) {
        const allDatasets = await listDatasets();
        setDataset(allDatasets.find((d) => d.fields.dataset_id === sf.dataset) || null);
      }

      // Fetch all collections available to this site (resolves dependencies)
      if (sf.site_id) {
        try {
          setSchemaNamespaces(await getSiteCollections(sf.site_id));
        } catch { setSchemaNamespaces([]); }
      }
    } catch (err) {
      setError(err.message);
    }
  }, [siteId]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteSite(siteId);
    navigate(project ? `/project/${project.id}` : '/');
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!site) return <p>Loading...</p>;

  const sf = site.fields;
  const totalCollections = schemaNamespaces.reduce((sum, ns) => sum + ns.collections.length, 0);

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {project && <> / <Link to={`/project/${project.id}`}>{project.fields.name || 'Unnamed'}</Link></>}
        {' / '}<strong>{sf.name || sf.site_id || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{sf.name || 'Unnamed Site'}</h2>
        <p className="project-ns">{sf.site_id || ''}</p>
        {sf.namespace && <p className="site-ns-detail">Namespace: <code>{sf.namespace}</code></p>}
        {dataset && (
          <p className="site-dataset-detail">
            Dataset: <Link to={`/dataset/${dataset.id}`} className="row-link">
              {dataset.fields.name || dataset.fields.dataset_id}
            </Link>
          </p>
        )}
        {project && (
          <p className="site-project-detail">
            Project: <Link to={`/project/${project.id}`} className="row-link">
              {project.fields.name || 'Unnamed'}
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
