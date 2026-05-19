import { useParams, useNavigate, useSearchParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getRecord, deleteRecord, listFields } from '../api.js';
import { displayValue } from './recordFields.jsx';

function formatDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

export default function RecordDetail() {
  const { user, project, version, name: collection, recordId } = useParams();
  const [searchParams] = useSearchParams();
  const siteName = searchParams.get('site');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const projectId = `${user}/${project}`;
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;
  const siteParam = siteName ? `?site=${encodeURIComponent(siteName)}` : '';

  const { data: record, isLoading, error } = useQuery({
    queryKey: ['record', projectId, siteName, collection, recordId],
    queryFn: () => getRecord(projectId, siteName, collection, recordId),
    enabled: !!siteName,
  });

  const { data: fields = [] } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  const remove = useMutation({
    mutationFn: () => deleteRecord(projectId, siteName, collection, recordId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['records', projectId, siteName, collection] });
      navigate(collectionPath);
    },
  });

  if (!siteName) {
    return <p className="error">Missing site context.</p>;
  }
  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading || !record) return <p>Loading...</p>;

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{record.id}</h2>
          <Link to={`${collectionPath}/records/${record.id}/edit${siteParam}`} className="btn">
            Edit
          </Link>
        </div>
        <p className="resource-id">{record.id}</p>
        <p className="detail-meta">Site: <code>{siteName}</code></p>
      </section>

      <section>
        <div className="section-heading">
          <h3>Fields <span className="count">({fields.length})</span></h3>
        </div>
        {fields.length === 0 ? (
          <p className="empty-state">No fields defined.</p>
        ) : (
          <div className="list">
            {fields.map((f) => (
              <div key={f.name} className="list-row">
                <div className="list-row-main">
                  <span className="list-row-name">{f.name}</span>
                  <span className="list-row-meta">{f.type}</span>
                  <span className="list-row-label">{displayValue(f, record.fields[f.name])}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section>
        <div className="section-heading">
          <h3>Metadata</h3>
        </div>
        <div className="list">
          <div className="list-row">
            <div className="list-row-main">
              <span className="list-row-name">created</span>
              <span className="list-row-label" title={record.created_at}>{formatDate(record.created_at)}</span>
              <span className="list-row-meta">by {record.created_by}</span>
            </div>
          </div>
          <div className="list-row">
            <div className="list-row-main">
              <span className="list-row-name">updated</span>
              <span className="list-row-label" title={record.updated_at}>{formatDate(record.updated_at)}</span>
              <span className="list-row-meta">by {record.updated_by}</span>
            </div>
          </div>
          <div className="list-row">
            <div className="list-row-main">
              <span className="list-row-name">owner</span>
              <span className="list-row-label">{record.owner}</span>
            </div>
          </div>
        </div>
      </section>

      <section className="danger-zone">
        <h3 className="danger-zone-heading">Danger zone</h3>
        <div className="danger-row">
          <div className="danger-row-info">
            <strong>Delete this record</strong>
            <p>The record will be permanently removed from the dataset.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete record
          </button>
        </div>
      </section>
    </>
  );
}
