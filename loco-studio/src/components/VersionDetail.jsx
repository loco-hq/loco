import { useParams, useNavigate, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getManifest, deleteVersion, listCollections,
} from '../api.js';

export default function VersionDetail() {
  const { user, project, version } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: manifest, isLoading, error } = useQuery({
    queryKey: ['manifest', user, project, version],
    queryFn: () => getManifest(user, project, version),
  });

  const { data: allCollections = [] } = useQuery({
    queryKey: ['collections', user, project, version],
    queryFn: () => listCollections(user, project, version),
  });

  const remove = useMutation({
    mutationFn: () => deleteVersion(user, project, version),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['versions', user, project] });
      navigate(`/projects/${user}/${project}`);
    },
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  const ownNs = `${user}/${project}`;
  const own = allCollections.filter((c) => c.project === ownNs && c.version === version);
  const inherited = allCollections.filter((c) => c.project !== ownNs || c.version !== version);

  return (
    <>
      <section className="detail-header">
        <div className="detail-header-row">
          <h2>{version}</h2>
        </div>
        <p className="resource-id">{ownNs} / versions / {version}</p>
        {manifest.dependencies.length > 0 && (
          <p className="detail-meta">
            Dependencies:{' '}
            {manifest.dependencies.map((d, i) => (
              <span key={d}>
                {i > 0 && ', '}<code>{d}</code>
              </span>
            ))}
          </p>
        )}
      </section>

      <section>
        <div className="section-heading">
          <h3>Collections <span className="count">({own.length})</span></h3>
          <div className="section-heading-actions">
            <Link
              to={`/projects/${user}/${project}/versions/${version}/collections/new`}
              className="btn btn-primary"
            >
              New collection
            </Link>
          </div>
        </div>
        {own.length === 0 ? (
          <p className="empty-state">No collections yet.</p>
        ) : (
          <div className="list">
            {own.map((c) => (
              <Link
                key={c.name}
                to={`/projects/${user}/${project}/versions/${version}/collections/${c.name}`}
                className="list-row"
              >
                <div className="list-row-main">
                  <span className="list-row-name">{c.name}</span>
                  {c.label && <span className="list-row-label">{c.label}</span>}
                </div>
              </Link>
            ))}
          </div>
        )}
      </section>

      {inherited.length > 0 && (
        <section>
          <div className="section-heading">
            <h3>Inherited collections <span className="count">({inherited.length})</span></h3>
          </div>
          <div className="list">
            {inherited.map((c) => (
              <div key={`${c.project}/${c.version}/${c.name}`} className="list-row">
                <div className="list-row-main">
                  <span className="list-row-name">{c.name}</span>
                  {c.label && <span className="list-row-label">{c.label}</span>}
                  <span className="list-row-meta">{c.project}@{c.version}</span>
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
            <strong>Delete this version</strong>
            <p>The manifest and every collection and field in this version will be permanently removed. Sites pointing at this version will break.</p>
          </div>
          <button className="delete-btn" onClick={() => remove.mutate()}>
            Delete version
          </button>
        </div>
      </section>
    </>
  );
}
