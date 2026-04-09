import { useState, useEffect, useCallback } from 'react';
import { useParams, Link } from 'react-router-dom';
import {
  getProject, listFields, createField, deleteField,
  listRecords, addRecord, updateRecord, deleteRecord,
} from '../api.js';

const FIELD_TYPES = ['string', 'integer', 'float', 'boolean'];

export default function DeckDetail() {
  const { '*': path } = useParams();
  const lastSlash = path.lastIndexOf('/');
  const projectId = path.slice(0, lastSlash);
  const deckName = path.slice(lastSlash + 1);

  const [project, setProject] = useState(null);
  const [fields, setFields] = useState([]);
  const [records, setRecords] = useState([]);
  const [error, setError] = useState(null);

  // New field form
  const [addingField, setAddingField] = useState(false);
  const [newFieldName, setNewFieldName] = useState('');
  const [newFieldType, setNewFieldType] = useState('string');

  // New record form
  const [addingRecord, setAddingRecord] = useState(false);
  const [newRecordFields, setNewRecordFields] = useState({});

  // Editing record
  const [editingId, setEditingId] = useState(null);
  const [editFields, setEditFields] = useState({});

  const parseNamespace = (ns) => {
    const [user, proj] = (ns || '').split('/');
    return { user, project: proj, version: '0.0.1-dev', siteId: 'dev' };
  };

  const load = useCallback(async () => {
    try {
      const proj = await getProject(projectId);
      setProject(proj);
      const { user, project: projSlug, version, siteId } = parseNamespace(proj.namespace);
      if (!user || !projSlug) return;

      const fieldList = await listFields(user, projSlug, version, deckName);
      setFields(fieldList);

      try {
        setRecords(await listRecords(user, projSlug, deckName, siteId));
      } catch {
        setRecords([]);
      }
    } catch (err) {
      setError(err.message);
    }
  }, [projectId, deckName]);

  useEffect(() => { load(); }, [load]);

  const fieldEntries = fields.map(([ns, f]) => ({
    key: ns,
    name: f.name || ns.split('/').pop(),
    type: f.type || 'string',
  }));

  // --- Field handlers ---

  const handleAddField = async (e) => {
    e.preventDefault();
    if (!newFieldName.trim() || !project) return;
    const slug = newFieldName.trim().toLowerCase().replace(/[^a-z0-9]+/g, '_');
    const { user, project: projSlug, version } = parseNamespace(project.namespace);
    try {
      await createField(user, projSlug, version, deckName, slug, newFieldType);
      setNewFieldName('');
      setNewFieldType('string');
      setAddingField(false);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const handleDeleteField = async (name) => {
    const { user, project: projSlug, version } = parseNamespace(project.namespace);
    try {
      await deleteField(user, projSlug, version, deckName, name);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  // --- Record handlers ---

  const handleAddRecord = async (e) => {
    e.preventDefault();
    const { user, project: projSlug, siteId } = parseNamespace(project.namespace);
    try {
      await addRecord(user, projSlug, deckName, siteId, newRecordFields);
      setNewRecordFields({});
      setAddingRecord(false);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const handleDeleteRecord = async (id) => {
    const { user, project: projSlug, siteId } = parseNamespace(project.namespace);
    try {
      await deleteRecord(user, projSlug, deckName, siteId, id);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const startEditing = (record) => {
    setEditingId(record.id);
    setEditFields({ ...record.fields });
  };

  const cancelEditing = () => {
    setEditingId(null);
    setEditFields({});
  };

  const handleSaveEdit = async (id) => {
    const { user, project: projSlug, siteId } = parseNamespace(project.namespace);
    try {
      await updateRecord(user, projSlug, deckName, siteId, id, editFields);
      setEditingId(null);
      setEditFields({});
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  if (error) return <p className="error-banner">Something went wrong: {error}</p>;
  if (!project) return <div className="loading">Drawing a card...</div>;

  return (
    <section className="deck-detail">
      <div className="breadcrumb-nav">
        <Link to="/" className="back-link">Projects</Link>
        <span className="breadcrumb-sep">&#8250;</span>
        <Link to={`/project/${projectId}`} className="back-link">{project.name}</Link>
        <span className="breadcrumb-sep">&#8250;</span>
        <span className="breadcrumb-current">{deckName}</span>
      </div>

      <div className="detail-card">
        <div className="detail-suit suit-spade">&#9824;</div>
        <h2 className="detail-name">{deckName}</h2>
        <p className="detail-ns">{project.namespace}.{deckName}</p>
      </div>

      {/* --- Properties --- */}
      <h3 className="section-title">
        Properties <span className="count">({fieldEntries.length})</span>
      </h3>
      <div className="properties-list">
        {fieldEntries.map((f) => (
          <div key={f.key} className="property-row">
            <span className="property-name">{f.name}</span>
            <span className="property-type">{f.type}</span>
            <button className="property-delete" onClick={() => handleDeleteField(f.name)} title="Remove property">&times;</button>
          </div>
        ))}
        {addingField ? (
          <form className="property-row property-add-form" onSubmit={handleAddField}>
            <input className="property-input" type="text" placeholder="Property name" value={newFieldName} onChange={(e) => setNewFieldName(e.target.value)} autoFocus />
            <select className="property-type-select" value={newFieldType} onChange={(e) => setNewFieldType(e.target.value)}>
              {FIELD_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
            </select>
            <button type="submit" className="btn-create btn-sm">Add</button>
            <button type="button" className="btn-cancel btn-sm" onClick={() => { setAddingField(false); setNewFieldName(''); }}>Cancel</button>
          </form>
        ) : (
          <button className="property-add-btn" onClick={() => setAddingField(true)}>+ Add property</button>
        )}
      </div>

      {/* --- Cards (records) --- */}
      <h3 className="section-title">
        Cards <span className="count">({records.length})</span>
      </h3>
      <div className="records-grid">
        {records.map((record) => (
          <div key={record.id} className="record-card">
            {editingId === record.id ? (
              /* Editing mode */
              <form className="record-edit-form" onSubmit={(e) => { e.preventDefault(); handleSaveEdit(record.id); }}>
                {fieldEntries.map((f) => (
                  <div key={f.name} className="record-field">
                    <label className="record-field-label">{f.name}</label>
                    <input
                      className="record-field-input"
                      type={f.type === 'integer' || f.type === 'float' ? 'number' : 'text'}
                      step={f.type === 'float' ? 'any' : undefined}
                      value={editFields[f.name] ?? ''}
                      onChange={(e) => setEditFields({ ...editFields, [f.name]: e.target.value })}
                    />
                  </div>
                ))}
                <div className="record-actions">
                  <button type="submit" className="btn-create btn-sm">Save</button>
                  <button type="button" className="btn-cancel btn-sm" onClick={cancelEditing}>Cancel</button>
                </div>
              </form>
            ) : (
              /* View mode */
              <>
                {fieldEntries.length === 0 ? (
                  <p className="record-no-props">No properties defined</p>
                ) : (
                  fieldEntries.map((f) => (
                    <div key={f.name} className="record-field">
                      <span className="record-field-label">{f.name}</span>
                      <span className="record-field-value">
                        {record.fields[f.name] != null ? String(record.fields[f.name]) : '\u2014'}
                      </span>
                    </div>
                  ))
                )}
                <div className="record-actions">
                  <button className="record-action-btn" onClick={() => startEditing(record)} title="Edit">Edit</button>
                  <button className="record-action-btn record-action-delete" onClick={() => handleDeleteRecord(record.id)} title="Delete">Delete</button>
                  <span className="record-meta">{record.id.slice(0, 8)}</span>
                </div>
              </>
            )}
          </div>
        ))}

        {/* Add card */}
        {addingRecord ? (
          <form className="record-card record-card-new" onSubmit={handleAddRecord}>
            {fieldEntries.length === 0 ? (
              <p className="record-no-props">Add properties first</p>
            ) : (
              fieldEntries.map((f) => (
                <div key={f.name} className="record-field">
                  <label className="record-field-label">{f.name}</label>
                  <input
                    className="record-field-input"
                    type={f.type === 'integer' || f.type === 'float' ? 'number' : 'text'}
                    step={f.type === 'float' ? 'any' : undefined}
                    value={newRecordFields[f.name] ?? ''}
                    onChange={(e) => setNewRecordFields({ ...newRecordFields, [f.name]: e.target.value })}
                  />
                </div>
              ))
            )}
            <div className="record-actions">
              <button type="submit" className="btn-create btn-sm" disabled={fieldEntries.length === 0}>Add</button>
              <button type="button" className="btn-cancel btn-sm" onClick={() => { setAddingRecord(false); setNewRecordFields({}); }}>Cancel</button>
            </div>
          </form>
        ) : (
          <button className="card card-add" onClick={() => setAddingRecord(true)}>
            <span className="card-add-plus">+</span>
            <span className="card-add-label">New Card</span>
          </button>
        )}
      </div>
    </section>
  );
}
