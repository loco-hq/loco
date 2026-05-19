import { Link, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { listRecords } from '../api.js';
import { displayValue } from './recordFields.jsx';

export default function RecordsTable({
  projectId,
  siteName,
  collection,
  fields,
  collectionPath,
}) {
  const navigate = useNavigate();
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
    <div className="records-table-wrap">
      <table className="records-table">
        <thead>
          <tr>
            <th className="records-th-id">ID</th>
            {fields.map((f) => (
              <th key={f.name} title={f.label || f.name}>
                {f.label || f.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {records.map((rec) => {
            const href = `${collectionPath}/records/${rec.id}?site=${siteParam}`;
            return (
              <tr
                key={rec.id}
                onClick={() => navigate(href)}
                className="records-row"
              >
                <td className="records-td-id">
                  <Link
                    to={href}
                    onClick={(e) => e.stopPropagation()}
                    className="records-id-link"
                  >
                    {rec.id}
                  </Link>
                </td>
                {fields.map((f) => {
                  const v = rec.fields[f.name];
                  const rendered = displayValue(f, v);
                  const isEmpty = rendered === '—';
                  return (
                    <td
                      key={f.name}
                      className={`records-td records-td-${f.type}${
                        isEmpty ? ' records-td-empty' : ''
                      }`}
                      title={isEmpty ? '' : rendered}
                    >
                      {rendered}
                    </td>
                  );
                })}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
