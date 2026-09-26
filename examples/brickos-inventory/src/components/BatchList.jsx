import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { BATCH_KINDS, useAdd, useRecords } from '../data.js';

export default function BatchList() {
  const batches = useRecords('batch');
  const lots = useRecords('lot');

  // Lot count and piece count per batch, from the one full lot list.
  const totals = new Map();
  for (const lot of lots.data ?? []) {
    const t = totals.get(lot.fields.batch_id) ?? { lots: 0, pieces: 0 };
    t.lots += 1;
    t.pieces += lot.fields.qty ?? 0;
    totals.set(lot.fields.batch_id, t);
  }

  const rows = [...(batches.data ?? [])].sort((a, b) =>
    String(a.fields.label).localeCompare(String(b.fields.label)),
  );

  return (
    <>
      <div className="page-head">
        <h1>Batches</h1>
      </div>
      <NewBatch />
      {batches.error && <p className="error">{batches.error.message}</p>}
      {batches.isPending ? (
        <p className="muted">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="empty">No batches yet. Create one above.</p>
      ) : (
        <table className="grid">
          <thead>
            <tr>
              <th>Label</th>
              <th>Kind</th>
              <th>Item no.</th>
              <th className="num">Lots</th>
              <th className="num">Pieces</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((b) => {
              const t = totals.get(b.id) ?? { lots: 0, pieces: 0 };
              return (
                <tr key={b.id}>
                  <td>
                    <Link to={`/batch/${b.id}`}>{b.fields.label || '(untitled)'}</Link>
                  </td>
                  <td>
                    <span className="tag">{b.fields.kind}</span>
                  </td>
                  <td className="mono">{b.fields.item_no}</td>
                  <td className="num">{t.lots}</td>
                  <td className="num">{t.pieces}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </>
  );
}

function NewBatch() {
  const navigate = useNavigate();
  const add = useAdd('batch');
  const [label, setLabel] = useState('');
  const [kind, setKind] = useState('inventory');
  const [itemNo, setItemNo] = useState('');

  function submit(e) {
    e.preventDefault();
    add.mutate(
      { label: label.trim(), kind, item_no: itemNo.trim() },
      { onSuccess: (rec) => navigate(`/batch/${rec.id}`) },
    );
  }

  return (
    <form className="card inline-form" onSubmit={submit}>
      <input placeholder="New batch label" value={label} onChange={(e) => setLabel(e.target.value)} />
      <input
        placeholder="Kind"
        list="batch-kinds"
        value={kind}
        onChange={(e) => setKind(e.target.value)}
      />
      <datalist id="batch-kinds">
        {BATCH_KINDS.map((k) => (
          <option key={k} value={k} />
        ))}
      </datalist>
      <input
        placeholder="Item no. (for a set or minifig)"
        value={itemNo}
        onChange={(e) => setItemNo(e.target.value)}
      />
      <button className="primary" disabled={!label.trim() || add.isPending}>
        Create batch
      </button>
      {add.error && <p className="error">{add.error.message}</p>}
    </form>
  );
}
