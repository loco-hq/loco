import { Link } from 'react-router-dom';
import { useRuns } from '../data.js';

export default function Runs() {
  const runs = useRuns();
  return (
    <>
      <div className="page-head row">
        <h1>Pull runs</h1>
        <Link className="button primary" to="/new">
          New pull run
        </Link>
      </div>
      {runs.error && <p className="error">{runs.error.message}</p>}
      {runs.isPending ? (
        <p className="muted">Loading…</p>
      ) : runs.data.length === 0 ? (
        <p className="empty">No runs yet.</p>
      ) : (
        <ul className="cards">
          {runs.data.map((r) => (
            <li key={r.id}>
              <Link to={`/run/${r.id}`} className="card run-card">
                <span>
                  <strong>{r.fields.label}</strong>
                  <span className="hint">{when(r.fields.created_at)}</span>
                </span>
                <span className={`tag ${r.fields.status}`}>{r.fields.status}</span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

export const when = (iso) =>
  iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '';
