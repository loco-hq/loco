import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import {
  getProject, deleteProject,
  listSitesForProject, addSite, deleteSite,
  listDatasetsForProject, addDataset, deleteDataset,
} from '../api.js';

export default function ProjectDetail() {
  const { '*': projectId } = useParams();
  const navigate = useNavigate();
  const [project, setProject] = useState(null);
  const [sites, setSites] = useState([]);
  const [datasets, setDatasets] = useState([]);
  const [error, setError] = useState(null);

  // "ben/crm/project" → user "ben", project "crm", projectPath "ben/crm"
  const [user, projectName] = projectId.split('/');
  const projectPath = `${user}/${projectName}`;

  const load = useCallback(async () => {
    try {
      const [proj, projectSites, projectDatasets] = await Promise.all([
        getProject(projectId),
        listSitesForProject(user, projectName),
        listDatasetsForProject(user, projectName),
      ]);
      setProject(proj);
      setSites(projectSites);
      setDatasets(projectDatasets);
    } catch (err) {
      setError(err.message);
    }
  }, [projectId, user, projectName]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteProject(projectId);
    navigate('/');
  };

  const handleAddSite = async (e) => {
    e.preventDefault();
    const form = e.target;
    const fields = {
      project: projectPath,
      name: form.elements.name.value,
      label: form.elements.label.value,
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
    await addDataset({
      project: projectPath,
      name: form.elements.name.value,
      label: form.elements.label.value,
      description: form.elements.description.value,
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

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link> / <strong>{project.label || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{project.label || 'Unnamed'}</h2>
        <p className="project-ns">{projectPath}</p>
        <p className="project-desc">{project.description || ''}</p>
        <button className="delete-btn" onClick={handleDelete}>Delete Project</button>
      </section>

      <section>
        <h3>Sites <span className="count">({sites.length})</span></h3>
        <form className="add-form" onSubmit={handleAddSite}>
          <input name="name" placeholder="Site name (e.g. acme-prod)" required />
          <input name="label" placeholder="Site label" required />
          <input name="version" placeholder="Version" defaultValue="0.0.1-dev" required />
          <select name="dataset">
            <option value="">No dataset</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
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
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
                {fields.dataset && <span className="site-dataset">dataset: {fields.dataset}</span>}
              </div>
              <button className="delete-btn" onClick={() => handleDeleteSite(id)}>delete</button>
            </div>
          ))}
        </div>
      </section>

      <section>
        <h3>Datasets <span className="count">({datasets.length})</span></h3>
        <form className="add-form" onSubmit={handleAddDataset}>
          <input name="name" placeholder="Dataset name (e.g. acme-prod)" required />
          <input name="label" placeholder="Dataset label" required />
          <input name="description" placeholder="Description" />
          <button type="submit">Add Dataset</button>
        </form>
        <div className="datasets-list">
          {datasets.length === 0 && <p className="empty-state">No datasets yet.</p>}
          {datasets.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/dataset/${id}`} className="row-link">
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
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
