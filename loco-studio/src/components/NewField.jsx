import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { TextField, SelectField } from 'loco-ui';
import { createField } from '../api.js';

const FIELD_TYPES = ['string', 'integer', 'float', 'boolean', 'list'];
const TYPE_OPTIONS = FIELD_TYPES.map((t) => ({ value: t, label: t }));

export default function NewField() {
  const { user, project, version, name: collection } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;

  const [name, setName] = useState('');
  const [label, setLabel] = useState('');
  const [type, setType] = useState('string');

  const create = useMutation({
    mutationFn: (body) => createField(user, project, version, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(collectionPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    create.mutate({ collection, name, type, label });
  };

  return (
    <div className="form-page">
      <h2>New field</h2>
      <p className="form-help">Add a typed field to <code>{collection}</code>.</p>
      <form onSubmit={handleSubmit}>
        <TextField
          label="Name"
          required
          pattern="[a-z][a-z0-9_]*"
          placeholder="e.g. title"
          value={name}
          onChange={setName}
        />
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
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(collectionPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create field</button>
        </div>
      </form>
    </div>
  );
}
