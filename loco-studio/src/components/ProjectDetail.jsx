import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import {
  getProject, deleteProject,
  listSites, addSite, deleteSite,
  listDatasets, addDataset, deleteDataset,
} from '../api.js';

export default function ProjectDetail() {
  const { projectId } = useParams();
  const navigate = useNavigate();
  const [project, setProject] = useState(null);
  const [sites, setSites] = useState([]);
  const [datasets, setDatasets] = useState([]);
  const [error, setError] = useState(null);

  const load = useCallback(async () => {
    try {
      const proj = await getProject(projectId);
      setProject(proj);

      const [allSites, allDatasets] = await Promise.all([listSites(), listDatasets()]);
      setSites(allSites.filter((s) => s.fields.project === projectId));
      setDatasets(allDatasets.filter((d) => d.fields.project === projectId));
    } catch (err) {
      setError(err.message);
    }
  }, [projectId]);

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
      project: projectId,
      namespace: form.elements.namespace.value,
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
    await addDataset({
      dataset_id: form.elements.dataset_id.value,
      name: form.elements.name.value,
      description: form.elements.description.value,
      project: projectId,
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

  const ns = project.fields.namespace || '';

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link> / <strong>{project.fields.name || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{project.fields.name || 'Unnamed'}</h2>
        <p className="project-ns">{ns}</p>
        <p className="project-desc">{project.fields.description || ''}</p>
        <button className="delete-btn" onClick={handleDelete}>Delete Project</button>
      </section>

      <section>
        <h3>Sites <span className="count">({sites.length})</span></h3>
        <form className="add-form" onSubmit={handleAddSite}>
          <input name="site_id" placeholder="Site ID (e.g. acme-prod)" required />
          <input name="name" placeholder="Site name" required />
          <input name="namespace" placeholder="Namespace@version" defaultValue={ns ? `${ns}@0.0.1-dev` : ''} required />
          <select name="dataset">
            <option value="">No dataset</option>
            {datasets.map((d) => (
              <option key={d.id} value={d.fields.dataset_id || ''}>
                {d.fields.name || d.fields.dataset_id}
              </option>
            ))}
          </select>
          <button type="submit">Add Site</button>
        </form>
        <div className="sites-list">
          {sites.length === 0 && <p className="empty-state">No sites yet.</p>}
          {sites.map((s) => (
            <div key={s.id} className="site-row">
              <div>
                <Link to={`/site/${s.id}`} className="row-link">
                  <strong>{s.fields.site_id || ''}</strong>
                </Link>
                <span className="site-name">{s.fields.name || ''}</span>
                {s.fields.dataset && <span className="site-dataset">dataset: {s.fields.dataset}</span>}
                {s.fields.namespace && <span className="site-ns">ns: {s.fields.namespace}</span>}
              </div>
              <button className="delete-btn" onClick={() => handleDeleteSite(s.id)}>delete</button>
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
          {datasets.map((d) => (
            <div key={d.id} className="site-row">
              <div>
                <Link to={`/dataset/${d.id}`} className="row-link">
                  <strong>{d.fields.dataset_id || ''}</strong>
                </Link>
                <span className="site-name">{d.fields.name || ''}</span>
                {d.fields.description && <span className="site-dataset">{d.fields.description}</span>}
              </div>
              <button className="delete-btn" onClick={() => handleDeleteDataset(d.id)}>delete</button>
            </div>
          ))}
        </div>
      </section>

    </>
  );
}
