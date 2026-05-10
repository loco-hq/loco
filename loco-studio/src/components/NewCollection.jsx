import { useParams, useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createCollection } from '../api.js';

export default function NewCollection() {
  const { user, project, version } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const versionPath = `/projects/${user}/${project}/versions/${version}`;

  const create = useMutation({
    mutationFn: (body) => createCollection(user, project, version, body),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['collections', user, project, version] });
      navigate(`${versionPath}/collections/${data.name}`);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    create.mutate({
      name: f.name.value,
      label: f.label.value,
      label_plural: f.label_plural.value,
    });
  };

  return (
    <div className="form-page">
      <h2>New collection</h2>
      <p className="form-help">A collection is a typed group of records, like a table.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" name="name" required pattern="[a-z][a-z0-9_]*" placeholder="e.g. task" />
          <span className="field-help">Lowercase, used as a path segment.</span>
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required placeholder="Display name (e.g. Task)" />
        </div>
        <div className="form-field">
          <label htmlFor="label_plural">Plural label</label>
          <input id="label_plural" name="label_plural" required placeholder="e.g. Tasks" />
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(versionPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create collection</button>
        </div>
      </form>
    </div>
  );
}
