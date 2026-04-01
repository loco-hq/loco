import { useState, useEffect, useCallback } from 'react';
import { useParams } from 'react-router-dom';
import { useSchema } from './SchemaContext.jsx';
import { listRecords, addRecord, deleteRecord } from '../loco.js';

function coerceValue(value, fieldType) {
  if (fieldType === 'integer') return parseInt(value, 10);
  if (fieldType === 'float') return parseFloat(value);
  if (fieldType === 'boolean') return value.toLowerCase() === 'true';
  return value;
}

export default function CollectionView() {
  const { user, project, name } = useParams();
  const { collections } = useSchema();
  const [records, setRecords] = useState(null);
  const [error, setError] = useState(null);

  const col = collections?.find((c) => c.user === user && c.project === project && c.name === name);

  const load = useCallback(async () => {
    if (!col) return;
    try {
      setRecords(await listRecords(col.user, col.project, col.name));
      setError(null);
    } catch (err) {
      setError(err.message);
    }
  }, [col]);

  useEffect(() => { load(); }, [load]);

  if (!col) return <p className="error">Collection not found</p>;
  if (error) return <p className="error">Error: {error}</p>;
  if (!records) return <p>Loading...</p>;

  const handleAdd = async (e) => {
    e.preventDefault();
    const form = e.target;
    const fields = {};
    col.fields.forEach((f) => {
      fields[f.name] = coerceValue(form.elements[f.name].value, f.type);
    });
    await addRecord(col.user, col.project, col.name, fields);
    form.reset();
    load();
  };

  const handleDelete = async (id) => {
    await deleteRecord(col.user, col.project, col.name, id);
    load();
  };

  return (
    <>
      <h2>{col.label} <span className="count">({records.length})</span></h2>
      <form onSubmit={handleAdd}>
        {col.fields.map((f) => (
          <input key={f.name} name={f.name} placeholder={f.name} required />
        ))}
        <button type="submit">Add</button>
      </form>
      <table>
        <thead>
          <tr>
            {col.fields.map((f) => <th key={f.name}>{f.name}</th>)}
            <th></th>
          </tr>
        </thead>
        <tbody>
          {records.map((r) => (
            <tr key={r.id}>
              {col.fields.map((f) => (
                <td key={f.name}>{r.fields[f.name] ?? ''}</td>
              ))}
              <td>
                <button className="delete" onClick={() => handleDelete(r.id)}>
                  delete
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}
