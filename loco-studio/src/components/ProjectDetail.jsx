import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import {
  getProject, deleteProject,
  listSites, addSite, deleteSite,
  listDatasets, addDataset, deleteDataset,
} from '../api.js';

export default function ProjectDetail() {
  const { '*': projectId } = useParams();
  const navigate = useNavigate();
  const [project, setProject] = useState(null);
  const [sites, setSites] = useState([]);
  const [datasets, setDatasets] = useState([]);
  const [error, setError] = useState(null);

  // Derive the namespace prefix from the project config ID
  // e.g. "projects/ben/crm/project" → "projects/ben/crm/"
  const nsPrefix = projectId.replace(/\/project$/, '/');

  const load = useCallback(async () => {
    try {
      const proj = await getProject(projectId);
      setProject(proj);

      const [allSites, allDatasets] = await Promise.all([listSites(), listDatasets()]);
      setSites(allSites.filter(([id]) => id.startsWith(nsPrefix + 'sites/')));
      setDatasets(allDatasets.filter(([id]) => id.startsWith(nsPrefix + 'datasets/')));
    } catch (err) {
      setError(err.message);
    }
  }, [projectId, nsPrefix]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteProject(projectId);
    navigate('/');
  };

  const handleAddSite = async (e) => {
    e.preventDefault();
    const form = e.target;
    const fields = {
      site_id: form.elements.site_id.value,
      name: form.elements.name.value,
      project: project.namespace.split('/').pop(),
      namespace: project.namespace,
      version: form.elements.version.value || '0.0.1-dev',
    };
    if (form.elements.dataset.value) {
      fields.dataset = form.elements.dataset.value;
    }
    await addSite(fields);
    form.reset();
    load();
  };

  const handleDeleteSite = async (id) => {
    await deleteSite(id);
    load();
  };

  const handleAddDataset = async (e) => {
    e.preventDefault();
    const form = e.target;
    await addDataset(project.namespace, {
      dataset_id: form.elements.dataset_id.value,
      name: form.elements.name.value,
      description: form.elements.description.value,
      project: project.namespace.split('/').pop(),
    });
    form.reset();
    load();
  };

  const handleDeleteDataset = async (id) => {
    await deleteDataset(id);
    load();
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!project) return <p>Loading...</p>;

  const ns = project.namespace || '';

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link> / <strong>{project.name || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{project.name || 'Unnamed'}</h2>
        <p className="project-ns">{ns}</p>
        <p className="project-desc">{project.description || ''}</p>
        <button className="delete-btn" onClick={handleDelete}>Delete Project</button>
      </section>

      <section>
        <h3>Sites <span className="count">({sites.length})</span></h3>
        <form className="add-form" onSubmit={handleAddSite}>
          <input name="site_id" placeholder="Site ID (e.g. acme-prod)" required />
          <input name="name" placeholder="Site name" required />
          <input name="version" placeholder="Version" defaultValue="0.0.1-dev" required />
          <select name="dataset">
            <option value="">No dataset</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.dataset_id || ''}>
                {fields.name || fields.dataset_id}
              </option>
            ))}
          </select>
          <button type="submit">Add Site</button>
        </form>
        <div className="sites-list">
          {sites.length === 0 && <p className="empty-state">No sites yet.</p>}
          {sites.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/site/${id}`} className="row-link">
                  <strong>{fields.site_id || ''}</strong>
                </Link>
                <span className="site-name">{fields.name || ''}</span>
                {fields.dataset && <span className="site-dataset">dataset: {fields.dataset}</span>}
                {fields.namespace && <span className="site-ns">ns: {fields.namespace}</span>}
              </div>
              <button className="delete-btn" onClick={() => handleDeleteSite(id)}>delete</button>
            </div>
          ))}
        </div>
      </section>

      <section>
        <h3>Datasets <span className="count">({datasets.length})</span></h3>
        <form className="add-form" onSubmit={handleAddDataset}>
          <input name="dataset_id" placeholder="Dataset ID (e.g. acme-prod)" required />
          <input name="name" placeholder="Dataset name" required />
          <input name="description" placeholder="Description" />
          <button type="submit">Add Dataset</button>
        </form>
        <div className="datasets-list">
          {datasets.length === 0 && <p className="empty-state">No datasets yet.</p>}
          {datasets.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/dataset/${id}`} className="row-link">
                  <strong>{fields.dataset_id || ''}</strong>
                </Link>
                <span className="site-name">{fields.name || ''}</span>
                {fields.description && <span className="site-dataset">{fields.description}</span>}
              </div>
              <button className="delete-btn" onClick={() => handleDeleteDataset(id)}>delete</button>
            </div>
          ))}
        </div>
      </section>

    </>
  );
}
