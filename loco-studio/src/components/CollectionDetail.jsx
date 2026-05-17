import { useState, useMemo } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getCollection, deleteCollection,
  listFields, listSites, listRecords,
  listFieldsets, updateFieldset,
} from '../api.js';
import RecordsTable from './RecordsTable.jsx';

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

  const { data: fieldsets = [] } = useQuery({
    queryKey: ['fieldsets', user, project, version, name],
    queryFn: () => listFieldsets(user, project, version, name),
  });

  // The first auto_add fieldset is the canonical order target. Backend
  // guarantees one exists once any field has been created; if a collection
  // is bare we just hide the reorder controls until then.
  const orderTarget = useMemo(
    () => fieldsets.find((fs) => fs.auto_add) || null,
    [fieldsets],
  );

  const reorder = useMutation({
    mutationFn: (nextFields) =>
      updateFieldset(user, project, version, name, orderTarget.name, {
        fields: nextFields,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['fieldsets', user, project, version, name] });
      qc.invalidateQueries({ queryKey: ['fields', user, project, version, name] });
    },
  });

  const moveField = (fieldName, delta) => {
    if (!orderTarget) return;
    const current = orderTarget.fields;
    const idx = current.indexOf(fieldName);
    if (idx < 0) return;
    const swap = idx + delta;
    if (swap < 0 || swap >= current.length) return;
    const next = current.slice();
    [next[idx], next[swap]] = [next[swap], next[idx]];
    reorder.mutate(next);
  };

  const { data: allSites = [] } = useQuery({
    queryKey: ['sites', user, project],
    queryFn: () => listSites(user, project),
  });

  const sitesForVersion = useMemo(
    () => allSites.filter(([, f]) => f.version === version),
    [allSites, version],
  );

  const [selectedSiteName, setSelectedSiteName] = useState(null);
  const selectedSite = useMemo(() => {
    if (sitesForVersion.length === 0) return null;
    const match = sitesForVersion.find(([, f]) => f.name === selectedSiteName);
    return match ? match[1] : sitesForVersion[0][1];
  }, [sitesForVersion, selectedSiteName]);

  const projectId = `${user}/${project}`;
  const { data: records = [] } = useQuery({
    queryKey: ['records', projectId, selectedSite?.name, name],
    queryFn: () => listRecords(projectId, selectedSite.name, name),
    enabled: !!selectedSite,
  });

  const remove = useMutation({
    mutationFn: () => deleteCollection(user, project, version, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['collections', user, project, version] });
      navigate(versionPath);
    },
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
          <Link
            to={`/projects/${user}/${project}/versions/${version}/collections/${name}/edit`}
            className="btn"
          >
            Edit
          </Link>
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
            {own.map((f) => {
              const orderIdx = orderTarget ? orderTarget.fields.indexOf(f.name) : -1;
              const canMoveUp = orderIdx > 0;
              const canMoveDown =
                orderIdx >= 0 && orderIdx < orderTarget.fields.length - 1;
              return (
                <div key={f.name} className="list-row">
                  <Link
                    to={`/projects/${user}/${project}/versions/${version}/collections/${name}/fields/${f.name}`}
                    className="list-row-main"
                  >
                    <span className="list-row-name">{f.name}</span>
                    {f.label && <span className="list-row-label">{f.label}</span>}
                    <span className="list-row-meta">{f.type}</span>
                  </Link>
                  {orderTarget && (
                    <div className="list-row-actions">
                      <button
                        type="button"
                        onClick={() => moveField(f.name, -1)}
                        disabled={!canMoveUp || reorder.isPending}
                        aria-label={`Move ${f.name} up`}
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        onClick={() => moveField(f.name, 1)}
                        disabled={!canMoveDown || reorder.isPending}
                        aria-label={`Move ${f.name} down`}
                      >
                        ↓
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
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
                  {f.label && <span className="list-row-label">{f.label}</span>}
                  <span className="list-row-meta">{f.type}</span>
                  <span className="list-row-meta">{f.project}@{f.version}</span>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      <section>
        <div className="section-heading">
          <h3>
            Records {selectedSite && <span className="count">({records.length})</span>}
            {selectedSite && (
              <span className="section-heading-meta">
                Site: <code>{selectedSite.name}</code> · Dataset:{' '}
                <code>{selectedSite.dataset || 'none'}</code>
              </span>
            )}
          </h3>
          <div className="section-heading-actions">
            {sitesForVersion.length > 1 && (
              <select
                value={selectedSite?.name || ''}
                onChange={(e) => setSelectedSiteName(e.target.value)}
              >
                {sitesForVersion.map(([id, f]) => (
                  <option key={id} value={f.name}>
                    {f.label || f.name} → {f.dataset || 'no dataset'}
                  </option>
                ))}
              </select>
            )}
            {selectedSite && allFields.length > 0 && (
              <Link
                to={`/projects/${user}/${project}/versions/${version}/collections/${name}/records/new?site=${encodeURIComponent(selectedSite.name)}`}
                className="btn btn-primary"
              >
                New {collection.label || collection.name}
              </Link>
            )}
          </div>
        </div>
        {sitesForVersion.length === 0 ? (
          <div className="empty-state">
            <p>No site connects this version to a dataset yet.</p>
            <Link
              to={`/projects/${user}/${project}/sites/new?version=${encodeURIComponent(version)}`}
              className="btn btn-primary"
            >
              Create a site for this version
            </Link>
          </div>
        ) : (
          <RecordsTable
            projectId={projectId}
            siteName={selectedSite.name}
            collection={name}
            fields={allFields}
            collectionPath={`/projects/${user}/${project}/versions/${version}/collections/${name}`}
          />
        )}
      </section>

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
