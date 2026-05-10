import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getDataset, deleteDataset, listSites } from '../api.js';

export default function DatasetDetail() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: dataset, isLoading, error } = useQuery({
    queryKey: ['dataset', user, project, name],
    queryFn: () => getDataset(user, project, name),
  });

  const { data: sites = [] } = useQuery({
    queryKey: ['sites', user, project],
    queryFn: () => listSites(user, project),
  });

  const remove = useMutation({
    mutationFn: () => deleteDataset(user, project, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['datasets', user, project] });
      navigate(`/projects/${user}/${project}`);
    },
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const linkedSites = sites.filter(([, fields]) => fields.dataset === dataset.name);

  return (
    <>
      <section className="detail-header">
        <h2>{dataset.label || 'Unnamed Dataset'}</h2>
        <p className="project-ns">{dataset.name || ''}</p>
        {dataset.description && <p className="project-desc">{dataset.description}</p>}
        <button className="delete-btn" onClick={() => remove.mutate()}>Delete Dataset</button>
      </section>

      <section>
        <h3>Linked Sites <span className="count">({linkedSites.length})</span></h3>
        <div className="sites-list">
          {linkedSites.length === 0 && <p className="empty-state">No sites use this dataset.</p>}
          {linkedSites.map(([id, fields]) => (
            <div key={id} className="site-row">
              <div>
                <Link to={`/projects/${user}/${project}/sites/${fields.name}`} className="row-link">
                  <strong>{fields.name || ''}</strong>
                </Link>
                <span className="site-name">{fields.label || ''}</span>
              </div>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}
