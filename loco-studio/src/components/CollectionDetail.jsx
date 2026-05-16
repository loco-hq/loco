import { useState, useMemo } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getCollection, deleteCollection,
  listFields, listSites, listRecords,
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
            {own.map((f) => (
              <Link
                key={f.name}
                to={`/projects/${user}/${project}/versions/${version}/collections/${name}/fields/${f.name}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{f.name}</span>
                  <span className="list-row-meta">{f.type}</span>
                </div>
              </Link>
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
