import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listFields, deleteField } from '../api.js';

export default function FieldDetail() {
  const { user, project, version, name: collection, fieldName } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;

  const { data: allFields = [], isLoading, error } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  const remove = useMutation({
    mutationFn: () => deleteField(user, project, version, collection, fieldName),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, collection] });
      navigate(collectionPath);
    },
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const ownNs = `${user}/${project}`;
  const field = allFields.find(
    (f) => f.name === fieldName && f.project === ownNs && f.version === version,
  );

  if (!field) {
    return (
      <section className="detail-header">
        <p className="error">Field not found.</p>
        <Link to={collectionPath} className="btn">Back to collection</Link>
      </section>
    );
  }

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{field.name}</h2>
          <Link
            to={`/projects/${user}/${project}/versions/${version}/collections/${collection}/fields/${fieldName}/edit`}
            className="btn"
          >
            Edit
          </Link>
        </div>
        <p className="resource-id">{field.name}</p>
        <p className="detail-meta">Type: <code>{field.type}</code></p>
      </section>

      <section className="danger-zone">
        <h3 className="danger-zone-heading">Danger zone</h3>
        <div className="danger-row">
          <div className="danger-row-info">
            <strong>Delete this field</strong>
            <p>Stored values for this field across all records will become orphaned.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete field
          </button>
        </div>
      </section>
    </>
  );
}
