import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getProject, deleteProject,
  listSites, deleteSite,
  listDatasets, deleteDataset,
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

  const removeProject = useMutation({
    mutationFn: () => deleteProject(user, project),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['projects'] });
      navigate('/');
    },
  });

  const removeSite = useMutation({
    mutationFn: (name) => deleteSite(user, project, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['sites', user, project] }),
  });

  const removeDataset = useMutation({
    mutationFn: (name) => deleteDataset(user, project, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['datasets', user, project] }),
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const projectPath = `${user}/${project}`;

  return (
    <>
      <section className="detail-header">
        <h2>{proj.label || 'Unnamed'}</h2>
        <p className="resource-id">{projectPath}</p>
        {proj.description && <p className="resource-desc">{proj.description}</p>}
        <button className="delete-btn" onClick={() => removeProject.mutate()}>Delete project</button>
      </section>

      <section>
        <div className="section-heading">
          <h3>Sites <span className="count">({sites.length})</span></h3>
          <div className="section-heading-actions">
            <Link to={`/projects/${user}/${project}/sites/new`} className="btn btn-primary">New site</Link>
          </div>
        </div>
        {sites.length === 0 ? (
          <p className="empty-state">No sites yet.</p>
        ) : (
          <div className="list">
            {sites.map(([id, fields]) => (
              <div key={id} className="list-row">
                <div className="list-row-main">
                  <Link
                    to={`/projects/${user}/${project}/sites/${fields.name}`}
                    className="list-row-name"
                  >
                    {fields.name}
                  </Link>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
                  {fields.dataset && <span className="list-row-meta">dataset: {fields.dataset}</span>}
                </div>
                <div className="list-row-actions">
                  <button className="delete-btn" onClick={() => removeSite.mutate(fields.name)}>Delete</button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section>
        <div className="section-heading">
          <h3>Datasets <span className="count">({datasets.length})</span></h3>
          <div className="section-heading-actions">
            <Link to={`/projects/${user}/${project}/datasets/new`} className="btn btn-primary">New dataset</Link>
          </div>
        </div>
        {datasets.length === 0 ? (
          <p className="empty-state">No datasets yet.</p>
        ) : (
          <div className="list">
            {datasets.map(([id, fields]) => (
              <div key={id} className="list-row">
                <div className="list-row-main">
                  <Link
                    to={`/projects/${user}/${project}/datasets/${fields.name}`}
                    className="list-row-name"
                  >
                    {fields.name}
                  </Link>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
                  {fields.description && <span className="list-row-desc">{fields.description}</span>}
                </div>
                <div className="list-row-actions">
                  <button className="delete-btn" onClick={() => removeDataset.mutate(fields.name)}>Delete</button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </>
  );
}
