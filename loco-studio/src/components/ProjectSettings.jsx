import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getProject, deleteProject,
  listSites, listDatasets, listVersions,
} from '../api.js';

export default function ProjectSettings() {
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

  const { data: versions = [] } = useQuery({
    queryKey: ['versions', user, project],
    queryFn: () => listVersions(user, project),
  });

  const removeProject = useMutation({
    mutationFn: () => deleteProject(user, project),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['projects'] });
      navigate('/');
    },
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const projectPath = `${user}/${project}`;

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{proj.label || 'Unnamed'}</h2>
          <Link to={`/projects/${user}/${project}/edit`} className="btn">Edit</Link>
        </div>
        <p className="resource-id">{projectPath}</p>
        {proj.description && <p className="resource-desc">{proj.description}</p>}
      </section>

      <section>
        <div className="section-heading">
          <h3>Versions <span className="count">({versions.length})</span></h3>
          <div className="section-heading-actions">
            <Link to={`/projects/${user}/${project}/versions/new`} className="btn btn-primary">New version</Link>
          </div>
        </div>
        {versions.length === 0 ? (
          <p className="empty-state">No versions yet.</p>
        ) : (
          <div className="list">
            {versions.map(([id, fields]) => (
              <Link
                key={id}
                to={`/projects/${user}/${project}/versions/${fields.version}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{fields.version}</span>
                  {fields.dependencies.length > 0 && (
                    <span className="list-row-meta">{fields.dependencies.length} deps</span>
                  )}
                </div>
              </Link>
            ))}
          </div>
        )}
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
              <Link
                key={id}
                to={`/projects/${user}/${project}/sites/${fields.name}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{fields.name}</span>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
                  {fields.dataset && <span className="list-row-meta">dataset: {fields.dataset}</span>}
                </div>
              </Link>
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
              <Link
                key={id}
                to={`/projects/${user}/${project}/datasets/${fields.name}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{fields.name}</span>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
                  {fields.description && <span className="list-row-desc">{fields.description}</span>}
                </div>
              </Link>
            ))}
          </div>
        )}
      </section>

      <section className="danger-zone">
        <h3 className="danger-zone-heading">Danger zone</h3>
        <div className="danger-row">
          <div className="danger-row-info">
            <strong>Delete this project</strong>
            <p>All sites and datasets in this project will be permanently removed.</p>
          </div>
          <button className="delete-btn" onClick={() => removeProject.mutate()}>
            Delete project
          </button>
        </div>
      </section>
    </>
  );
}
