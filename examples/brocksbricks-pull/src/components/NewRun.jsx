import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { PULL_STATUSES, readPlan, useCreateRun } from '../data.js';

// Reading orders is a button, not a page load: each order's lines are one
// BrickLink call against the store's daily quota. The plan is read once and
// held here until it is written or abandoned.
export default function NewRun() {
  const navigate = useNavigate();
  const read = useMutation({ mutationFn: readPlan });
  const create = useCreateRun();
  const [progress, setProgress] = useState(null);
  const [label, setLabel] = useState(() =>
    new Date().toLocaleString(undefined, { weekday: 'short', day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' }),
  );
  const plan = read.data;

  function write() {
    create.mutate(
      { plan, label: label.trim(), onProgress: (done, total) => setProgress({ done, total }) },
      { onSuccess: ({ run }) => navigate(`/run/${run.id}`) },
    );
  }

  return (
    <>
      <div className="crumbs">
        <Link to="/">Pull runs</Link> / New
      </div>
      <div className="page-head">
        <h1>New pull run</h1>
      </div>

      {!plan && (
        <div className="card stack">
          <p>
            Reads {PULL_STATUSES.join(', ')} orders and their lines from BrickLink, skips orders
            already in an open run, and groups the rest by item, color, condition, and location.
          </p>
          <button className="primary big" disabled={read.isPending} onClick={() => read.mutate()}>
            {read.isPending ? 'Reading orders…' : 'Read orders'}
          </button>
          {read.error && <p className="error">{read.error.message}</p>}
        </div>
      )}

      {plan && (
        <>
          <div className="card stack">
            <div className="stats">
              <span>
                <strong>{plan.included.length}</strong> orders
              </span>
              <span>
                <strong>{plan.lines.length}</strong> lines
              </span>
            </div>
            {plan.skipped.length > 0 && (
              <p className="muted">
                Skipped {plan.skipped.length} order{plan.skipped.length === 1 ? '' : 's'} already in an
                open run.
              </p>
            )}
            <div className="stations">
              <span className="tag scale">Scale {plan.count.scale}</span>
              <span className="tag hand">Hand {plan.count.hand}</span>
              <span className="tag unlocated">Unlocated {plan.count.unlocated}</span>
            </div>
            <p className="hint">
              Scale at {plan.threshold} or more. {plan.locations} locations on file.
            </p>
            {plan.lines.length > 0 && (
              <>
                <label>
                  Label
                  <input value={label} onChange={(e) => setLabel(e.target.value)} />
                </label>
                <button className="primary big" disabled={create.isPending || !label.trim()} onClick={write}>
                  {create.isPending && progress
                    ? `Writing ${progress.done} of ${progress.total}…`
                    : 'Create run'}
                </button>
              </>
            )}
            {plan.lines.length === 0 && <p className="empty">Nothing to pull.</p>}
            {create.error && <p className="error">{create.error.message}</p>}
          </div>

          {plan.count.unlocated > 0 && (
            <div className="card">
              <h2>Unlocated</h2>
              <p className="hint">These remarks match no location code. They stay on the run.</p>
              <ul className="plain">
                {plan.lines
                  .filter((l) => l.station === 'unlocated')
                  .map((l) => (
                    <li key={l.key}>
                      <span className="mono">{l.item_no}</span> {l.color_name} × {l.qty} —{' '}
                      <span className="remark">{l.remarks || '(no remark)'}</span>
                    </li>
                  ))}
              </ul>
            </div>
          )}
        </>
      )}
    </>
  );
}
