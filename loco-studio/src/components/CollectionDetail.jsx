import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getCollection, deleteCollection,
  listFields, deleteField,
} from '../api.js';

export default function CollectionDetail() {
  const { user, project, version, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const versionPath = `/projects/${user}/${project}/versions/${version}`;

  const { data: collection, isLoading, error } = useQuery({
    queryKey: ['collection', user, project, version, name],
    queryFn: () => getCollection(user, project, version, name),
  });

  const { data: allFields = [] } = useQuery({
    queryKey: ['fields', user, project, version, name],
    queryFn: () => listFields(user, project, version, name),
  });

  const remove = useMutation({
    mutationFn: () => deleteCollection(user, project, version, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['collections', user, project, version] });
      navigate(versionPath);
    },
  });

  const removeField = useMutation({
    mutationFn: (fieldName) => deleteField(user, project, version, name, fieldName),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['fields', user, project, version, name] }),
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const ownNs = `${user}/${project}`;
  const own = allFields.filter((f) => f.project === ownNs && f.version === version);
  const inherited = allFields.filter((f) => f.project !== ownNs || f.version !== version);

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{collection.label || collection.name}</h2>
        </div>
        <p className="resource-id">{collection.name}</p>
        {collection.label_plural && (
          <p className="detail-meta">Plural: <code>{collection.label_plural}</code></p>
        )}
      </section>

      <section>
        <div className="section-heading">
          <h3>Fields <span className="count">({own.length})</span></h3>
          <div className="section-heading-actions">
            <Link
              to={`/projects/${user}/${project}/versions/${version}/collections/${name}/fields/new`}
              className="btn btn-primary"
            >
              New field
            </Link>
          </div>
        </div>
        {own.length === 0 ? (
          <p className="empty-state">No fields yet.</p>
        ) : (
          <div className="list">
            {own.map((f) => (
              <div key={f.name} className="list-row">
                <div className="list-row-main">
                  <span className="list-row-name">{f.name}</span>
                  <span className="list-row-meta">{f.type}</span>
                </div>
                <div className="list-row-actions">
                  <button className="delete-btn" onClick={() => removeField.mutate(f.name)}>
                    Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {inherited.length > 0 && (
        <section>
          <div className="section-heading">
            <h3>Inherited fields <span className="count">({inherited.length})</span></h3>
          </div>
          <div className="list">
            {inherited.map((f) => (
              <div key={`${f.project}/${f.version}/${f.name}`} className="list-row">
                <div className="list-row-main">
                  <span className="list-row-name">{f.name}</span>
                  <span className="list-row-meta">{f.type}</span>
                  <span className="list-row-meta">{f.project}@{f.version}</span>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="danger-zone">
        <h3 className="danger-zone-heading">Danger zone</h3>
        <div className="danger-row">
          <div className="danger-row-info">
            <strong>Delete this collection</strong>
            <p>All fields and stored records in this collection will be permanently removed.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete collection
          </button>
        </div>
      </section>
    </>
  );
}
