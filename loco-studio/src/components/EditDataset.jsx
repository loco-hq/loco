import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getDataset, updateDataset } from '../api.js';

export default function EditDataset() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const datasetPath = `/projects/${user}/${project}/datasets/${name}`;

  const { data: dataset, isLoading, error } = useQuery({
    queryKey: ['dataset', user, project, name],
    queryFn: () => getDataset(user, project, name),
  });

  const update = useMutation({
    mutationFn: (patch) => updateDataset(user, project, name, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['dataset', user, project, name] });
      qc.invalidateQueries({ queryKey: ['datasets', user, project] });
      navigate(datasetPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    update.mutate({
      label: f.label.value,
      description: f.description.value,
    });
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <div className="form-page">
      <h2>Edit dataset</h2>
      <p className="form-help">Dataset name is immutable. Update the label or description.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" value={dataset.name} disabled />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required defaultValue={dataset.label || ''} />
        </div>
        <div className="form-field">
          <label htmlFor="description">Description</label>
          <input id="description" name="description" defaultValue={dataset.description || ''} />
        </div>
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(datasetPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
