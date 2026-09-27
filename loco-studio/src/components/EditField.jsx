import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { TextField, SelectField } from 'loco-ui';
import { listFields, updateField } from '../api.js';
import { FIELD_TYPES, TYPE_OPTIONS } from '../fieldTypes.js';

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
  // A field saved before the server checked types may have one it no longer
  // accepts. Show it so the select is not blank, and send `type` only when
  // it changes, so the label can still be edited.
  const typeOptions = FIELD_TYPES.includes(field.type)
    ? TYPE_OPTIONS
    : [{ value: field.type, label: field.type }, ...TYPE_OPTIONS];

  const update = useMutation({
    mutationFn: (patch) => updateField(user, project, version, collection, fieldName, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(fieldPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    update.mutate(type === field.type ? { label } : { type, label });
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
          options={typeOptions}
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
