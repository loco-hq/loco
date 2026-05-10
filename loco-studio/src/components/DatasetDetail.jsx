import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getDataset, deleteDataset, getProject, listSitesForProject } from '../api.js';

export default function DatasetDetail() {
  const { '*': datasetId } = useParams();
  const navigate = useNavigate();
  const [dataset, setDataset] = useState(null);
  const [project, setProject] = useState(null);
  const [linkedSites, setLinkedSites] = useState([]);
  const [error, setError] = useState(null);

  // "ben/crm/datasets/acme" → user="ben", projectName="crm"
  const [user, projectName] = datasetId.split('/');
  const projectConfigId = `${user}/${projectName}/project`;

  const load = useCallback(async () => {
    try {
      const d = await getDataset(datasetId);
      setDataset(d);

      const [proj, projectSites] = await Promise.all([
        getProject(projectConfigId).catch(() => null),
        listSitesForProject(user, projectName),
      ]);
      setProject(proj);
      setLinkedSites(projectSites.filter(([, fields]) => fields.dataset === d.name));
    } catch (err) {
      setError(err.message);
    }
  }, [datasetId, projectConfigId, user, projectName]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteDataset(datasetId);
    navigate(`/project/${projectConfigId}`);
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!dataset) return <p>Loading...</p>;

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {project && (
          <> / <Link to={`/project/${projectConfigId}`}>{project.label || 'Unnamed'}</Link></>
        )}
        {' / '}<strong>{dataset.label || dataset.name || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{dataset.label || 'Unnamed Dataset'}</h2>
        <p className="project-ns">{dataset.name || ''}</p>
        {dataset.description && <p className="project-desc">{dataset.description}</p>}
        {project && (
          <p className="site-project-detail">
            Project: <Link to={`/project/${projectConfigId}`} className="row-link">
              {project.label || 'Unnamed'}
            </Link>
          </p>
        )}
        <button className="delete-btn" onClick={handleDelete}>Delete Dataset</button>
      </section>

      <section>
        <h3>Linked Sites <span className="count">({linkedSites.length})</span></h3>
        <div className="sites-list">
          {linkedSites.length === 0 && <p className="empty-state">No sites use this dataset.</p>}
          {linkedSites.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/site/${id}`} className="row-link">
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
                {fields.project && <span className="site-ns">project: {fields.project}</span>}
              </div>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
