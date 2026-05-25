import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { TextField, SelectField } from 'loco-ui';
import { listFields, updateField } from '../api.js';

const FIELD_TYPES = ['string', 'integer', 'float', 'boolean', 'list'];
const TYPE_OPTIONS = FIELD_TYPES.map((t) => ({ value: t, label: t }));

export default function EditField() {
  const { user, project, version, name: collection, fieldName } = useParams();

  const { data: allFields = [], isLoading, error } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const ownNs = `${user}/${project}`;
  const field = allFields.find(
    (f) => f.name === fieldName && f.project === ownNs && f.version === version,
  );

  if (!field) return <p className="error">Field not found.</p>;

  return <EditFieldForm field={field} />;
}

function EditFieldForm({ field }) {
  const { user, project, version, name: collection, fieldName } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const fieldPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}/fields/${fieldName}`;

  const [label, setLabel] = useState(field.label || '');
  const [type, setType] = useState(field.type);

  const update = useMutation({
    mutationFn: (patch) => updateField(user, project, version, collection, fieldName, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(fieldPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    update.mutate({ type, label });
  };

  return (
    <div className="form-page">
      <h2>Edit field</h2>
      <p className="form-help">Field name is immutable. Update the label or type.</p>
      <form onSubmit={handleSubmit}>
        <TextField label="Name" value={field.name} onChange={() => {}} disabled />
        <TextField
          label="Label"
          placeholder="e.g. Title"
          value={label}
          onChange={setLabel}
        />
        <SelectField
          label="Type"
          required
          options={TYPE_OPTIONS}
          value={type}
          onChange={setType}
        />
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(fieldPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
