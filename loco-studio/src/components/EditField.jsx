import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listFields, updateField } from '../api.js';

const FIELD_TYPES = ['string', 'integer', 'float', 'boolean', 'list'];

export default function EditField() {
  const { user, project, version, name: collection, fieldName } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const fieldPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}/fields/${fieldName}`;

  const { data: allFields = [], isLoading, error } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  const update = useMutation({
    mutationFn: (patch) => updateField(user, project, version, collection, fieldName, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(fieldPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    update.mutate({ type: f.type.value });
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const ownNs = `${user}/${project}`;
  const field = allFields.find(
    (f) => f.name === fieldName && f.project === ownNs && f.version === version,
  );

  if (!field) {
    return <p className="error">Field not found.</p>;
  }

  return (
    <div className="form-page">
      <h2>Edit field</h2>
      <p className="form-help">Field name is immutable. Update the type.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" value={field.name} disabled />
        </div>
        <div className="form-field">
          <label htmlFor="type">Type</label>
          <select id="type" name="type" required defaultValue={field.type}>
            {FIELD_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
          </select>
        </div>
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(fieldPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
