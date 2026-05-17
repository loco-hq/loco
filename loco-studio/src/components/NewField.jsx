import { useParams, useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { createField } from '../api.js';

const FIELD_TYPES = ['string', 'integer', 'float', 'boolean', 'list'];

export default function NewField() {
  const { user, project, version, name: collection } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;

  const create = useMutation({
    mutationFn: (body) => createField(user, project, version, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(collectionPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    create.mutate({
      collection,
      name: f.name.value,
      type: f.type.value,
      label: f.label.value,
    });
  };

  return (
    <div className="form-page">
      <h2>New field</h2>
      <p className="form-help">Add a typed field to <code>{collection}</code>.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" name="name" required pattern="[a-z][a-z0-9_]*" placeholder="e.g. title" />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" placeholder="e.g. Title" />
        </div>
        <div className="form-field">
          <label htmlFor="type">Type</label>
          <select id="type" name="type" required defaultValue="string">
            {FIELD_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
          </select>
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(collectionPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create field</button>
        </div>
      </form>
    </div>
  );
}
