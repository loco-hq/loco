import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getProject, deleteProject,
  listSites, createSite, deleteSite,
  listDatasets, createDataset, deleteDataset,
} from '../api.js';

export default function ProjectDetail() {
  const { user, project } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: proj, isLoading, error } = useQuery({
    queryKey: ['project', user, project],
    queryFn: () => getProject(user, project),
  });

  const { data: sites = [] } = useQuery({
    queryKey: ['sites', user, project],
    queryFn: () => listSites(user, project),
  });

  const { data: datasets = [] } = useQuery({
    queryKey: ['datasets', user, project],
    queryFn: () => listDatasets(user, project),
  });

  const invalidate = (key) => qc.invalidateQueries({ queryKey: [key, user, project] });

  const removeProject = useMutation({
    mutationFn: () => deleteProject(user, project),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['projects'] });
      navigate('/');
    },
  });

  const addSite = useMutation({
    mutationFn: (body) => createSite(user, project, body),
    onSuccess: () => invalidate('sites'),
  });

  const removeSite = useMutation({
    mutationFn: (name) => deleteSite(user, project, name),
    onSuccess: () => invalidate('sites'),
  });

  const addDataset = useMutation({
    mutationFn: (body) => createDataset(user, project, body),
    onSuccess: () => invalidate('datasets'),
  });

  const removeDataset = useMutation({
    mutationFn: (name) => deleteDataset(user, project, name),
    onSuccess: () => invalidate('datasets'),
  });

  const handleAddSite = (e) => {
    e.preventDefault();
    const form = e.target;
    addSite.mutate(
      {
        name: form.elements.name.value,
        label: form.elements.label.value,
        version: form.elements.version.value || '0.0.1-dev',
        dataset: form.elements.dataset.value || '',
      },
      { onSuccess: () => form.reset() },
    );
  };

  const handleAddDataset = (e) => {
    e.preventDefault();
    const form = e.target;
    addDataset.mutate(
      {
        name: form.elements.name.value,
        label: form.elements.label.value,
        description: form.elements.description.value || '',
      },
      { onSuccess: () => form.reset() },
    );
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const projectPath = `${user}/${project}`;

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link> / <strong>{proj.label || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{proj.label || 'Unnamed'}</h2>
        <p className="project-ns">{projectPath}</p>
        <p className="project-desc">{proj.description || ''}</p>
        <button className="delete-btn" onClick={() => removeProject.mutate()}>Delete Project</button>
      </section>

      <section>
        <h3>Sites <span className="count">({sites.length})</span></h3>
        <form className="add-form" onSubmit={handleAddSite}>
          <input name="name" placeholder="Site name (e.g. acme-prod)" required />
          <input name="label" placeholder="Site label" required />
          <input name="version" placeholder="Version" defaultValue="0.0.1-dev" required />
          <select name="dataset" defaultValue="">
            <option value="">No dataset</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
              </option>
            ))}
          </select>
          <button type="submit" disabled={addSite.isPending}>Add Site</button>
        </form>
        {addSite.error && <p className="error">{addSite.error.message}</p>}
        <div className="sites-list">
          {sites.length === 0 && <p className="empty-state">No sites yet.</p>}
          {sites.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/projects/${user}/${project}/sites/${fields.name}`} className="row-link">
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
                {fields.dataset && <span className="site-dataset">dataset: {fields.dataset}</span>}
              </div>
              <button className="delete-btn" onClick={() => removeSite.mutate(fields.name)}>delete</button>
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
          <button type="submit" disabled={addDataset.isPending}>Add Dataset</button>
        </form>
        {addDataset.error && <p className="error">{addDataset.error.message}</p>}
        <div className="datasets-list">
          {datasets.length === 0 && <p className="empty-state">No datasets yet.</p>}
          {datasets.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/projects/${user}/${project}/datasets/${fields.name}`} className="row-link">
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
                {fields.description && <span className="site-dataset">{fields.description}</span>}
              </div>
              <button className="delete-btn" onClick={() => removeDataset.mutate(fields.name)}>delete</button>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
