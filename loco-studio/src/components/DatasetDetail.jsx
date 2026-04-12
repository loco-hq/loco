import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getDataset, deleteDataset, listProjects, listSites } from '../api.js';

export default function DatasetDetail() {
  const { '*': datasetId } = useParams();
  const navigate = useNavigate();
  const [dataset, setDataset] = useState(null);
  const [projectEntry, setProjectEntry] = useState(null);
  const [linkedSites, setLinkedSites] = useState([]);
  const [error, setError] = useState(null);

  // "ben/crm/datasets/acme" → projectConfigId "ben/crm/project", nsPrefix "ben/crm/"
  const projectConfigId = datasetId.replace(/\/datasets\/.*$/, '/project');
  const nsPrefix = datasetId.replace(/\/datasets\/.*$/, '/');

  const load = useCallback(async () => {
    try {
      const d = await getDataset(datasetId);
      setDataset(d);

      const [allProjects, allSites] = await Promise.all([listProjects(), listSites()]);

      const proj = allProjects.find(([id]) => id === projectConfigId);
      if (proj) setProjectEntry(proj);

      setLinkedSites(allSites.filter(([id, fields]) =>
        id.startsWith(nsPrefix + 'sites/') && fields.dataset === d.name
      ));
    } catch (err) {
      setError(err.message);
    }
  }, [datasetId, projectConfigId, nsPrefix]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    await deleteDataset(datasetId);
    navigate(projectEntry ? `/project/${projectEntry[0]}` : '/');
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!dataset) return <p>Loading...</p>;

  return (
    <>
      <div className="breadcrumb">
        <Link to="/">Projects</Link>
        {projectEntry && (
          <> / <Link to={`/project/${projectEntry[0]}`}>{projectEntry[1].label || 'Unnamed'}</Link></>
        )}
        {' / '}<strong>{dataset.label || dataset.name || 'Unnamed'}</strong>
      </div>

      <section className="detail-header">
        <h2>{dataset.label || 'Unnamed Dataset'}</h2>
        <p className="project-ns">{dataset.name || ''}</p>
        {dataset.description && <p className="project-desc">{dataset.description}</p>}
        {projectEntry && (
          <p className="site-project-detail">
            Project: <Link to={`/project/${projectEntry[0]}`} className="row-link">
              {projectEntry[1].label || 'Unnamed'}
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
