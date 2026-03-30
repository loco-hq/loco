import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getDataset, deleteDataset, getProject, listSites } from '../api.js';

export default function DatasetDetail() {
  const { datasetId } = useParams();
  const navigate = useNavigate();
  const [dataset, setDataset] = useState(null);
  const [project, setProject] = useState(null);
  const [linkedSites, setLinkedSites] = useState([]);
  const [error, setError] = useState(null);

  const load = useCallback(async () => {
    try {
      const d = await getDataset(datasetId);
      setDataset(d);
      const df = d.fields;

      if (df.project) {
        try { setProject(await getProject(df.project)); } catch { /* ignore */ }
      }

      const allSites = await listSites();
      setLinkedSites(allSites.filter((s) => s.fields.dataset === df.dataset_id));
    } catch (err) {
      setError(err.message);
    }
  }, [datasetId]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteDataset(datasetId);
    navigate(project ? `/project/${project.id}` : '/');
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!dataset) return <p>Loading...</p>;

  const df = dataset.fields;

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {project && <> / <Link to={`/project/${project.id}`}>{project.fields.name || 'Unnamed'}</Link></>}
        {' / '}<strong>{df.name || df.dataset_id || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{df.name || 'Unnamed Dataset'}</h2>
        <p className="project-ns">{df.dataset_id || ''}</p>
        {df.description && <p className="project-desc">{df.description}</p>}
        {project && (
          <p className="site-project-detail">
            Project: <Link to={`/project/${project.id}`} className="row-link">
              {project.fields.name || 'Unnamed'}
            </Link>
          </p>
        )}
        <button className="delete-btn" onClick={handleDelete}>Delete Dataset</button>
      </section>

      <section>
        <h3>Linked Sites <span className="count">({linkedSites.length})</span></h3>
        <div className="sites-list">
          {linkedSites.length === 0 && <p className="empty-state">No sites use this dataset.</p>}
          {linkedSites.map((s) => (
            <div key={s.id} className="site-row">
              <div>
                <Link to={`/site/${s.id}`} className="row-link">
                  <strong>{s.fields.site_id || ''}</strong>
                </Link>
                <span className="site-name">{s.fields.name || ''}</span>
                {s.fields.namespace && <span className="site-ns">ns: {s.fields.namespace}</span>}
              </div>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
