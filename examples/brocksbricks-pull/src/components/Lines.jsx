import { useState } from 'react';
import { useOutletContext, useParams } from 'react-router-dom';
import { useSetPicked, useSplits } from '../data.js';

const CONDITION = { N: 'New', U: 'Used' };

// One station's route, in walk order. Tapping a line picks it in full, or
// un-picks it; "Count" records a short pick.
export default function Lines() {
  const { id, station } = useParams();
  const { lines, open } = useOutletContext();
  const splits = useSplits(id);
  const setPicked = useSetPicked(id);
  const shown = lines.filter((l) => l.fields.station === station);

  if (shown.length === 0) return <p className="empty">No {station} lines on this run.</p>;
  return (
    <>
      {setPicked.error && <p className="error">{setPicked.error.message}</p>}
      <ol className="lines">
        {shown.map((l) => (
          <Line
            key={l.id}
            line={l}
            split={splits.data?.get(l.id) ?? []}
            open={open}
            pending={setPicked.isPending && setPicked.variables?.id === l.id}
            onPick={(picked) => setPicked.mutate({ id: l.id, picked })}
          />
        ))}
      </ol>
    </>
  );
}

function Line({ line, split, open, pending, onPick }) {
  const f = line.fields;
  const picked = f.picked_qty != null;
  const short = picked && f.picked_qty !== f.qty;
  const [counting, setCounting] = useState(false);
  const [count, setCount] = useState('');

  function submitCount(e) {
    e.preventDefault();
    const n = Number.parseInt(count, 10);
    if (Number.isInteger(n) && n >= 0) onPick(n);
    setCounting(false);
  }

  return (
    <li className={`line ${picked ? 'picked' : ''} ${short ? 'short' : ''}`}>
      <button
        className="line-main"
        disabled={!open || pending}
        onClick={() => onPick(picked ? null : f.qty)}
        aria-pressed={picked}
      >
        <span className="where">
          {f.station === 'unlocated' ? (
            <span className="remark">{f.remarks || '(no remark)'}</span>
          ) : (
            <span className="mono loc">{f.location_code}</span>
          )}
        </span>
        <span className="what">
          <span className="mono item">{f.item_no}</span>
          <span className="muted">
            {f.color_name ?? `color ${f.color_id}`} · {CONDITION[f.condition] ?? f.condition}
            {f.item_type !== 'PART' && ` · ${f.item_type}`}
          </span>
        </span>
        <span className="qty">
          {picked && short ? (
            <>
              <strong>{f.picked_qty}</strong>
              <span className="of">of {f.qty}</span>
            </>
          ) : (
            <strong>{f.qty}</strong>
          )}
          {picked && !short && <span className="check" aria-label="picked">✓</span>}
        </span>
      </button>
      <div className="line-foot">
        <ul className="split" aria-label="per-order share">
          {split.map((a) => (
            <li key={a.bl_order_id}>
              <span className="mono">#{a.bl_order_id}</span> <strong>{a.qty}</strong>
            </li>
          ))}
        </ul>
        {open &&
          (counting ? (
            <form className="count-form" onSubmit={submitCount}>
              <input
                type="number"
                inputMode="numeric"
                min="0"
                autoFocus
                value={count}
                onChange={(e) => setCount(e.target.value)}
                aria-label={`Picked of ${f.qty}`}
              />
              <button className="primary small">Save</button>
              <button type="button" className="small" onClick={() => setCounting(false)}>
                Cancel
              </button>
            </form>
          ) : (
            <button
              className="link count-link"
              onClick={() => {
                setCount(String(f.picked_qty ?? f.qty));
                setCounting(true);
              }}
            >
              Count
            </button>
          ))}
      </div>
    </li>
  );
}
