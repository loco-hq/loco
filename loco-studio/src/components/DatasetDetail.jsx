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
        <div className="detail-header-row">
          <h2>{dataset.label || 'Unnamed Dataset'}</h2>
          <Link to={`/projects/${user}/${project}/datasets/${name}/edit`} className="btn">Edit</Link>
        </div>
        <p className="resource-id">{dataset.name}</p>
        {dataset.description && <p className="resource-desc">{dataset.description}</p>}
      </section>

      <section>
        <div className="section-heading">
          <h3>Linked sites <span className="count">({linkedSites.length})</span></h3>
        </div>
        {linkedSites.length === 0 ? (
          <p className="empty-state">No sites use this dataset.</p>
        ) : (
          <div className="list">
            {linkedSites.map(([id, fields]) => (
              <Link
                key={id}
                to={`/projects/${user}/${project}/sites/${fields.name}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{fields.name}</span>
                  {fields.label && <span className="list-row-label">{fields.label}</span>}
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
            <strong>Delete this dataset</strong>
            <p>All records in this dataset will be permanently removed. Sites referencing it will need a different dataset.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete dataset
          </button>
        </div>
      </section>
    </>
  );
}
