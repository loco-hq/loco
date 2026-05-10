import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getProject, updateProject } from '../api.js';

export default function EditProject() {
  const { user, project } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const projectPath = `/projects/${user}/${project}`;

  const { data: proj, isLoading, error } = useQuery({
    queryKey: ['project', user, project],
    queryFn: () => getProject(user, project),
  });

  const update = useMutation({
    mutationFn: (patch) => updateProject(user, project, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['project', user, project] });
      qc.invalidateQueries({ queryKey: ['projects'] });
      navigate(projectPath);
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
      <h2>Edit project</h2>
      <p className="form-help">Project name is immutable. Update the label or description.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" value={`${user}/${project}`} disabled />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required defaultValue={proj.label || ''} />
        </div>
        <div className="form-field">
          <label htmlFor="description">Description</label>
          <input id="description" name="description" defaultValue={proj.description || ''} />
        </div>
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(projectPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
