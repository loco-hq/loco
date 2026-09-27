import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import * as api from '../api.js';
import {
  BATCH_KINDS,
  CONDITIONS,
  useAdd,
  useCatalog,
  useLots,
  useRemove,
  useRemoveBatch,
  useUpdate,
} from '../data.js';
import EditCell from './EditCell.jsx';

export default function BatchPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const batch = useQuery({ queryKey: ['batch', id], queryFn: () => api.get('batch', id) });
  const lots = useLots(id);
  const { colorLabel, itemLabel } = useCatalog();
  const updateBatch = useUpdate('batch');
  const updateLot = useUpdate('lot');
  const removeLot = useRemove('lot');
  const removeBatch = useRemoveBatch();

  if (batch.isPending) return <p className="muted">Loading…</p>;
  if (batch.error) return <p className="error">{batch.error.message}</p>;

  const b = batch.data.fields;
  const saveBatch = (field) => (value) => updateBatch.mutate({ id, fields: { [field]: value } });
  const saveLot = (lotId, field) => (value) =>
    updateLot.mutate({ id: lotId, fields: { [field]: value } });
  const pieces = lots.data.reduce((n, l) => n + (l.fields.qty ?? 0), 0);
  const writeError = updateBatch.error ?? updateLot.error ?? removeLot.error ?? removeBatch.error;

  function deleteBatch() {
    const n = lots.data.length;
    if (!confirm(`Delete “${b.label}” and its ${n} lot${n === 1 ? '' : 's'}?`)) return;
    removeBatch.mutate(id, { onSuccess: () => navigate('/') });
  }

  return (
    <>
      <nav className="crumbs">
        <Link to="/">Batches</Link> / <span>{b.label || '(untitled)'}</span>
      </nav>

      <section className="card batch-head">
        <label>
          Label
          <EditCell value={b.label} onSave={saveBatch('label')} />
        </label>
        <label>
          Kind
          <EditCell value={b.kind} list="batch-kinds" onSave={saveBatch('kind')} />
          <datalist id="batch-kinds">
            {BATCH_KINDS.map((k) => (
              <option key={k} value={k} />
            ))}
          </datalist>
        </label>
        <label>
          Item no.
          <EditCell value={b.item_no} onSave={saveBatch('item_no')} placeholder="—" />
          {b.item_no && itemLabel.get(b.item_no) && (
            <span className="hint">{itemLabel.get(b.item_no)}</span>
          )}
        </label>
        <div className="stats">
          <div>
            <strong>{lots.data.length}</strong> lots
          </div>
          <div>
            <strong>{pieces}</strong> pieces
          </div>
        </div>
      </section>

      {writeError && <p className="error">{writeError.message}</p>}

      <table className="grid lots">
        <thead>
          <tr>
            <th>Item no.</th>
            <th>Color</th>
            <th>Condition</th>
            <th className="num">Qty</th>
            <th aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {lots.data.map((lot) => {
            const f = lot.fields;
            return (
              <tr key={lot.id}>
                <td>
                  <EditCell value={f.item_no} className="mono" onSave={saveLot(lot.id, 'item_no')} />
                  {itemLabel.get(f.item_no) && <span className="hint">{itemLabel.get(f.item_no)}</span>}
                </td>
                <td>
                  <EditCell
                    value={f.color_code}
                    kind="int"
                    className="mono short"
                    onSave={saveLot(lot.id, 'color_code')}
                  />
                  {colorLabel.get(f.color_code) && (
                    <span className="hint">{colorLabel.get(f.color_code)}</span>
                  )}
                </td>
                <td>
                  <EditCell
                    value={f.condition}
                    kind="select"
                    options={CONDITIONS}
                    onSave={saveLot(lot.id, 'condition')}
                  />
                </td>
                <td className="num">
                  <EditCell
                    value={f.qty}
                    kind="int"
                    className="num short"
                    onSave={saveLot(lot.id, 'qty')}
                  />
                </td>
                <td className="actions">
                  <button
                    className="icon"
                    title="Delete lot"
                    onClick={() => removeLot.mutate(lot.id)}
                    disabled={removeLot.isPending}
                  >
                    ×
                  </button>
                </td>
              </tr>
            );
          })}
          <AddLotRow batchId={id} lots={lots.data} />
        </tbody>
      </table>
      {lots.data.length === 0 && !lots.isPending && (
        <p className="empty">No lots yet. Add the first one in the row above.</p>
      )}

      <div className="danger-zone">
        <button className="danger" onClick={deleteBatch} disabled={removeBatch.isPending}>
          Delete batch
        </button>
      </div>
    </>
  );
}

const EMPTY_LOT = { item_no: '', color_code: '', condition: 'new', qty: '1' };

/**
 * A lot is one quantity of a lot key (item_no, color_code, condition). Adding
 * a key the batch already holds adds to that lot instead of making a second
 * one (docs/brickos.md).
 */
function AddLotRow({ batchId, lots }) {
  const [lot, setLot] = useState(EMPTY_LOT);
  const [error, setError] = useState(null);
  const add = useAdd('lot');
  const update = useUpdate('lot');
  const set = (field) => (e) => setLot({ ...lot, [field]: e.target.value });

  function submit(e) {
    e.preventDefault();
    const item_no = lot.item_no.trim();
    const color_code = parseInt(lot.color_code, 10);
    const qty = parseInt(lot.qty, 10);
    if (!item_no) return setError('Item no. is required');
    if (!Number.isInteger(color_code)) return setError('Color must be a BrickLink color id');
    if (!Number.isInteger(qty) || qty <= 0) return setError('Qty must be a positive whole number');
    setError(null);

    const done = { onSuccess: () => setLot({ ...EMPTY_LOT, condition: lot.condition }) };
    const same = lots.find(
      (l) =>
        l.fields.item_no === item_no &&
        l.fields.color_code === color_code &&
        l.fields.condition === lot.condition,
    );
    if (same) {
      update.mutate({ id: same.id, fields: { qty: (same.fields.qty ?? 0) + qty } }, done);
    } else {
      add.mutate({ batch_id: batchId, item_no, color_code, condition: lot.condition, qty }, done);
    }
  }

  const busy = add.isPending || update.isPending;
  const message = error ?? add.error?.message ?? update.error?.message;
  return (
    <>
      <tr className="add-row">
        <td>
          <input
            form="add-lot"
            className="mono"
            placeholder="3001"
            value={lot.item_no}
            onChange={set('item_no')}
          />
        </td>
        <td>
          <input
            form="add-lot"
            className="mono short"
            placeholder="11"
            inputMode="numeric"
            value={lot.color_code}
            onChange={set('color_code')}
          />
        </td>
        <td>
          <select form="add-lot" value={lot.condition} onChange={set('condition')}>
            {CONDITIONS.map((c) => (
              <option key={c}>{c}</option>
            ))}
          </select>
        </td>
        <td className="num">
          <input
            form="add-lot"
            className="num short"
            inputMode="numeric"
            value={lot.qty}
            onChange={set('qty')}
          />
        </td>
        <td className="actions">
          <form id="add-lot" onSubmit={submit}>
            <button className="primary small" disabled={busy}>
              Add
            </button>
          </form>
        </td>
      </tr>
      {message && (
        <tr>
          <td colSpan={5} className="error">
            {message}
          </td>
        </tr>
      )}
    </>
  );
}
