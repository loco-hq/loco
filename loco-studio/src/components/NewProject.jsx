import { useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createProject } from '../api.js';

export default function NewProject() {
  const navigate = useNavigate();
  const qc = useQueryClient();

  const create = useMutation({
    mutationFn: createProject,
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['projects'] });
      const [u, p] = data.project.split('/');
      navigate(`/projects/${u}/${p}`);
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

  return (
    <div className="form-page">
      <h2>New project</h2>
      <p className="form-help">A project owns a set of schemas, sites, and datasets.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" name="name" required pattern="[a-z][a-z0-9_-]*" placeholder="e.g. crm" />
          <span className="field-help">Lowercase letters, digits, hyphens, underscores. Used as a path segment.</span>
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required placeholder="Display name (e.g. CRM)" />
        </div>
        <div className="form-field">
          <label htmlFor="description">Description</label>
          <input id="description" name="description" placeholder="Optional" />
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate('/')}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create project</button>
        </div>
      </form>
    </div>
  );
}
