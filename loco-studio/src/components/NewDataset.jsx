import { useParams, useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createDataset } from '../api.js';

export default function NewDataset() {
  const { user, project } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const create = useMutation({
    mutationFn: (body) => createDataset(user, project, body),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['datasets', user, project] });
      navigate(`/projects/${user}/${project}/datasets/${data.name}`);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    create.mutate({
      name: f.name.value,
      label: f.label.value,
      description: f.description.value,
    });
  };

  const projectPath = `/projects/${user}/${project}`;

  return (
    <div className="form-page">
      <h2>New dataset</h2>
      <p className="form-help">A dataset stores records for a collection in this project.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" name="name" required pattern="[a-z][a-z0-9_-]*" placeholder="e.g. prod" />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required placeholder="Display name" />
        </div>
        <div className="form-field">
          <label htmlFor="description">Description</label>
          <input id="description" name="description" placeholder="Optional" />
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(projectPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create dataset</button>
        </div>
      </form>
    </div>
  );
}
