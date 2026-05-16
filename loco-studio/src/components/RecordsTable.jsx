import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  listRecords, addRecord, updateRecord, deleteRecord,
} from '../api.js';

function initialDraft(fields) {
  const d = {};
  for (const f of fields) {
    d[f.name] = f.type === 'boolean' ? false : '';
  }
  return d;
}

function coerce(field, raw) {
  if (field.type === 'boolean') return !!raw;
  if (field.type === 'integer') {
    if (raw === '' || raw === null || raw === undefined) return null;
    const n = parseInt(raw, 10);
    return Number.isNaN(n) ? null : n;
  }
  if (field.type === 'float') {
    if (raw === '' || raw === null || raw === undefined) return null;
    const n = parseFloat(raw);
    return Number.isNaN(n) ? null : n;
  }
  return raw ?? '';
}

function buildPayload(fields, draft) {
  const out = {};
  for (const f of fields) {
    out[f.name] = coerce(f, draft[f.name]);
  }
  return out;
}

function FieldInput({ field, value, onChange }) {
  if (field.type === 'boolean') {
    return (
      <input
        type="checkbox"
        checked={!!value}
        onChange={(e) => onChange(e.target.checked)}
      />
    );
  }
  if (field.type === 'integer' || field.type === 'float') {
    return (
      <input
        type="number"
        step={field.type === 'integer' ? '1' : 'any'}
        value={value ?? ''}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }
  return (
    <input
      type="text"
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function displayValue(field, value) {
  if (value === null || value === undefined) return <span className="cell-null">—</span>;
  if (field.type === 'boolean') return value ? '✓' : '✗';
  if (value === '') return <span className="cell-null">—</span>;
  return String(value);
}

function RecordRow({ record, fields, onSave, onDelete, saving }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(record.fields);

  const startEdit = () => {
    setDraft(record.fields);
    setEditing(true);
  };
  const cancel = () => {
    setDraft(record.fields);
    setEditing(false);
  };
  const save = async () => {
    await onSave(record.id, buildPayload(fields, draft));
    setEditing(false);
  };

  if (editing) {
    return (
      <tr>
        {fields.map((f) => (
          <td key={f.name}>
            <FieldInput
              field={f}
              value={draft[f.name]}
              onChange={(v) => setDraft({ ...draft, [f.name]: v })}
            />
          </td>
        ))}
        <td className="row-actions">
          <button onClick={save} disabled={saving}>Save</button>
          <button onClick={cancel} disabled={saving}>Cancel</button>
        </td>
      </tr>
    );
  }
  return (
    <tr>
      {fields.map((f) => (
        <td key={f.name}>{displayValue(f, record.fields[f.name])}</td>
      ))}
      <td className="row-actions">
        <button onClick={startEdit}>Edit</button>
        <button className="delete-btn" onClick={() => onDelete(record.id)}>Delete</button>
      </td>
    </tr>
  );
}

function NewRow({ fields, onCreate, onCancel, saving }) {
  const [draft, setDraft] = useState(() => initialDraft(fields));
  const save = async () => {
    await onCreate(buildPayload(fields, draft));
    setDraft(initialDraft(fields));
  };
  return (
    <tr>
      {fields.map((f) => (
        <td key={f.name}>
          <FieldInput
            field={f}
            value={draft[f.name]}
            onChange={(v) => setDraft({ ...draft, [f.name]: v })}
          />
        </td>
      ))}
      <td className="row-actions">
        <button onClick={save} disabled={saving}>Save</button>
        <button onClick={onCancel} disabled={saving}>Cancel</button>
      </td>
    </tr>
  );
}

export default function RecordsTable({ projectId, siteName, collection, fields, adding, onCancelAdd }) {
  const qc = useQueryClient();
  const [opError, setOpError] = useState(null);

  const recordsKey = ['records', projectId, siteName, collection];

  const { data: records = [], isLoading, error } = useQuery({
    queryKey: recordsKey,
    queryFn: () => listRecords(projectId, siteName, collection),
  });

  const invalidate = () => qc.invalidateQueries({ queryKey: recordsKey });

  const add = useMutation({
    mutationFn: (payload) => addRecord(projectId, siteName, collection, payload),
    onSuccess: () => { setOpError(null); onCancelAdd(); invalidate(); },
    onError: (e) => setOpError(e.message),
  });

  const upd = useMutation({
    mutationFn: ({ id, payload }) => updateRecord(projectId, siteName, collection, id, payload),
    onSuccess: () => { setOpError(null); invalidate(); },
    onError: (e) => setOpError(e.message),
  });

  const del = useMutation({
    mutationFn: (id) => deleteRecord(projectId, siteName, collection, id),
    onSuccess: () => { setOpError(null); invalidate(); },
    onError: (e) => setOpError(e.message),
  });

  if (fields.length === 0) {
    return <p className="empty-state">Add a field above to start storing data.</p>;
  }

  if (error) return <p className="error">Error loading records: {error.message}</p>;

  return (
    <div className="records-table-wrap">
      {opError && <p className="error">{opError}</p>}
      <table className="records-table">
        <thead>
          <tr>
            {fields.map((f) => (
              <th key={f.name}>{f.name}</th>
            ))}
            <th className="row-actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {adding && (
            <NewRow
              fields={fields}
              onCreate={(payload) => add.mutateAsync(payload)}
              onCancel={() => { onCancelAdd(); setOpError(null); }}
              saving={add.isPending}
            />
          )}
          {records.map((rec) => (
            <RecordRow
              key={rec.id}
              record={rec}
              fields={fields}
              onSave={(id, payload) => upd.mutateAsync({ id, payload })}
              onDelete={(id) => {
                if (window.confirm('Delete this record?')) del.mutate(id);
              }}
              saving={upd.isPending || del.isPending}
            />
          ))}
          {!adding && records.length === 0 && (
            <tr>
              <td colSpan={fields.length + 1} className="records-empty">
                {isLoading ? 'Loading…' : 'No records yet.'}
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
