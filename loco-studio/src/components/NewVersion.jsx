import { useParams, useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createVersion } from '../api.js';

export default function NewVersion() {
  const { user, project } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const settingsPath = `/projects/${user}/${project}/settings`;

  const create = useMutation({
    mutationFn: (body) => createVersion(user, project, body),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['versions', user, project] });
      navigate(`/projects/${user}/${project}/versions/${data.version}`);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    create.mutate({ version: f.version.value });
  };

  return (
    <div className="form-page">
      <h2>New version</h2>
      <p className="form-help">
        A version groups collections and fields. A version with a hyphen (e.g. <code>0-draft</code>) is editable; without one it's published and read-only.
      </p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="version">Version</label>
          <input id="version" name="version" required pattern="[a-z0-9._-]+" placeholder="e.g. 0-draft" />
          <span className="field-help">Lowercase, single path segment.</span>
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(settingsPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create version</button>
        </div>
      </form>
    </div>
  );
}
