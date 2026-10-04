import { Link, NavLink, Outlet, useNavigate, useParams } from 'react-router-dom';
import { STATIONS } from '../plan.js';
import { useLines, useMarkDone, useRun } from '../data.js';
import { when } from './Runs.jsx';

const TITLES = { scale: 'Scale', hand: 'Hand', unlocated: 'Unlocated' };

export default function Run() {
  const { id } = useParams();
  const navigate = useNavigate();
  const run = useRun(id);
  const lines = useLines(id);
  const markDone = useMarkDone();

  const tally = Object.fromEntries(STATIONS.map((s) => [s, { lines: 0, picked: 0 }]));
  for (const l of lines.data) {
    tally[l.fields.station].lines += 1;
    if (l.fields.picked_qty != null) tally[l.fields.station].picked += 1;
  }
  const picked = lines.data.filter((l) => l.fields.picked_qty != null).length;
  const open = run.data?.fields.status === 'open';

  return (
    <>
      <div className="crumbs">
        <Link to="/">Pull runs</Link>
      </div>
      {run.error && <p className="error">{run.error.message}</p>}
      {run.data && (
        <div className="page-head row">
          <div>
            <h1>{run.data.fields.label}</h1>
            <span className="hint">
              {when(run.data.fields.created_at)} · {picked} of {lines.data.length} picked
            </span>
          </div>
          {open ? (
            <button
              disabled={markDone.isPending}
              onClick={() => markDone.mutate(id, { onSuccess: () => navigate('/') })}
            >
              Mark done
            </button>
          ) : (
            <span className="tag done">done</span>
          )}
        </div>
      )}
      {markDone.error && <p className="error">{markDone.error.message}</p>}
      <nav className="tabs">
        {STATIONS.map((s) => (
          <NavLink key={s} to={s} className={`tab ${s}`}>
            {TITLES[s]}
            <span className="count">
              {tally[s].picked}/{tally[s].lines}
            </span>
          </NavLink>
        ))}
      </nav>
      {lines.error && <p className="error">{lines.error.message}</p>}
      {lines.isPending ? <p className="muted">Loading…</p> : <Outlet context={{ lines: lines.data, open }} />}
    </>
  );
}
