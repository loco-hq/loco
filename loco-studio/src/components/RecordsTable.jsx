import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { listRecords } from '../api.js';
import { displayValue } from './recordFields.jsx';

function previewText(record, fields) {
  for (const f of fields) {
    if (f.type === 'string' || f.type === 'list') {
      const v = record.fields[f.name];
      if (v !== null && v !== undefined && v !== '') return String(v);
    }
  }
  return null;
}

export default function RecordsTable({
  projectId,
  siteName,
  collection,
  fields,
  collectionPath,
}) {
  const { data: records = [], isLoading, error } = useQuery({
    queryKey: ['records', projectId, siteName, collection],
    queryFn: () => listRecords(projectId, siteName, collection),
  });

  if (fields.length === 0) {
    return <p className="empty-state">Add a field above to start storing data.</p>;
  }
  if (error) return <p className="error">Error loading records: {error.message}</p>;
  if (isLoading) return <p className="empty-state">Loading…</p>;
  if (records.length === 0) {
    return <p className="empty-state">No records yet.</p>;
  }

  const siteParam = encodeURIComponent(siteName);

  return (
    <div className="list">
      {records.map((rec) => {
        const preview = previewText(rec, fields);
        const summary = fields
          .slice(0, 3)
          .map((f) => `${f.name}: ${displayValue(f, rec.fields[f.name])}`)
          .join(' · ');
        return (
          <Link
            key={rec.id}
            to={`${collectionPath}/records/${rec.id}?site=${siteParam}`}
            className="list-row"
          >
            <div className="list-row-main">
              <span className="list-row-name">{rec.id}</span>
              {preview && <span className="list-row-label">{preview}</span>}
              <span className="list-row-meta">{summary}</span>
            </div>
          </Link>
        );
      })}
    </div>
  );
}
