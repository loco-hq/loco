import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { createClient } from 'loco-client';
import { allocationFields, buildPlan, compareLines, pickLineFields } from './plan.js';

// Hosted at dev.orders.brocksbricks.<host>, the server infers the site from
// the Host header. Under `vite dev` the page is on localhost:5178 and the
// proxy reaches the apex, which has no site, so the client names it.
export const loco = createClient(
  import.meta.env.DEV ? { projectId: 'brocksbricks/orders', siteId: 'dev' } : {},
);

// The installed BrickLink store's live collections (docs/brocksbricks.md).
// Their fields are loco/bricklink's, so a query names them qualified; the
// records come back with bare field names.
const ORDERS = 'loco/bricklink.store:orders';
const ORDER_ITEMS = 'loco/bricklink.store:order_items';
const bl = (field) => `loco/bricklink.${field}`;

/** BrickLink statuses a pull run picks from. */
export const PULL_STATUSES = ['PENDING'];

/** How many writes a new run keeps in flight at once. */
const WRITE_CONCURRENCY = 6;

const where = (field, op, value) => ({ field, op, value });

/**
 * Everything a new run is planned from, read once. BrickLink is read live and
 * every order's lines are one upstream call, so this is never a render-time
 * query: the New run page calls it from a button.
 */
export async function readPlan() {
  const [settings, locations, openRuns, orders] = await Promise.all([
    // One record is the contract; if there are more, the oldest wins and
    // the page says so.
    loco.queryAll({
      collection: 'settings',
      fields: ['scale_threshold'],
      order: [{ field: '$created_at' }],
    }),
    loco.queryAll({ collection: 'location', fields: ['code', 'walk_order'] }),
    loco.queryAll({ collection: 'pull_run', where: where('status', 'eq', 'open'), fields: [] }),
    loco.queryAll({ collection: ORDERS, where: where(bl('status'), 'in', PULL_STATUSES), fields: [] }),
  ]);
  const threshold = settings[0]?.fields.scale_threshold;
  const settingsCount = settings.length;
  if (threshold == null) {
    throw new Error('No settings record. Add one with a scale_threshold before planning a run.');
  }
  const allocated = openRuns.length
    ? await loco.queryAll({
        collection: 'pick_allocation',
        where: where('pull_run_id', 'in', openRuns.map((r) => r.id)),
        fields: ['bl_order_id'],
      })
    : [];
  const taken = new Set(allocated.map((a) => a.fields.bl_order_id));
  // Only the orders this run will take: an order already in an open run
  // costs no upstream call for its lines.
  const fresh = orders.filter((o) => !taken.has(o.id));
  // 500 lines a page, and the source pages over a full upstream read, so
  // past 500 lines each further page reads every order again (#141).
  const items = fresh.length
    ? await loco.queryAll({
        collection: ORDER_ITEMS,
        where: where(bl('order_id'), 'in', fresh.map((o) => o.id)),
      })
    : [];
  const plan = buildPlan({ orders, items, locations, threshold, allocated: taken });
  return { ...plan, threshold, settingsCount, locations: locations.length };
}

/** Runs `tasks` with at most `limit` in flight, stopping at the first failure. */
async function pool(tasks, limit) {
  let next = 0;
  let failed = null;
  async function worker() {
    while (next < tasks.length && !failed) {
      const task = tasks[next++];
      try {
        await task();
      } catch (err) {
        failed ??= err;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, tasks.length) }, worker));
  if (failed) throw failed;
}

/**
 * Writes a plan as one `pull_run`, a `pick_line` per line, and a
 * `pick_allocation` per order line. /data has no batch insert, so that is
 * 1 + lines + allocations requests. The lake has no transactions either: a
 * failure leaves what was written, and the run is open, so its orders are
 * skipped by the next plan until the run is marked done. Both wait on
 * batch writes (#142); nothing here works around them.
 */
export function useCreateRun() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ plan, label, onProgress }) => {
      const total = 1 + plan.lines.length + plan.lines.reduce((n, l) => n + l.allocations.length, 0);
      let done = 0;
      const tick = () => onProgress?.(++done, total);
      const run = await loco.data('pull_run').add({
        label,
        created_at: new Date().toISOString().replace(/\.\d+Z$/, 'Z'),
        status: 'open',
      });
      tick();
      try {
        await pool(
          plan.lines.map((line) => async () => {
            const added = await loco.data('pick_line').add(pickLineFields(run.id, line));
            tick();
            for (const alloc of line.allocations) {
              await loco.data('pick_allocation').add(allocationFields(run.id, added.id, alloc));
              tick();
            }
          }),
          WRITE_CONCURRENCY,
        );
      } catch (err) {
        throw new Error(
          `${err.message}. Run "${label}" is partial: ${done} of ${total} records written. ` +
            'Mark it done to release its orders, then plan again.',
        );
      }
      return { run, requests: total };
    },
    onSettled: () => qc.invalidateQueries({ queryKey: ['pull_run'] }),
  });
}

export const useRuns = () =>
  useQuery({
    queryKey: ['pull_run'],
    queryFn: () =>
      loco.queryAll({
        collection: 'pull_run',
        order: [{ field: 'status', dir: 'desc' }, { field: 'created_at', dir: 'desc' }],
      }),
  });

export const useRun = (id) =>
  useQuery({ queryKey: ['pull_run', id], queryFn: () => loco.data('pull_run').get(id) });

/** A run's lines in route order. One read for every station. */
export function useLines(runId) {
  const q = useQuery({
    queryKey: ['pick_line', runId],
    queryFn: async () => {
      const lines = await loco.queryAll({
        collection: 'pick_line',
        where: where('pull_run_id', 'eq', runId),
      });
      return lines.sort((a, b) => compareLines(a.fields, b.fields));
    },
  });
  return { ...q, data: q.data ?? [] };
}

/**
 * pick_line id → one `{ bl_order_id, qty }` per order, from one query over
 * the run. A lot in both batches of one order is two allocations; the picker
 * bags it once, so the share is summed here.
 */
export function useSplits(runId) {
  return useQuery({
    queryKey: ['pick_allocation', runId],
    queryFn: async () => {
      const allocs = await loco.queryAll({
        collection: 'pick_allocation',
        where: where('pull_run_id', 'eq', runId),
        fields: ['pick_line_id', 'bl_order_id', 'qty'],
        order: [{ field: 'bl_order_id', collation: 'natural' }],
      });
      const byLine = new Map();
      for (const { fields: a } of allocs) {
        const shares = byLine.get(a.pick_line_id) ?? new Map();
        shares.set(a.bl_order_id, (shares.get(a.bl_order_id) ?? 0) + a.qty);
        byLine.set(a.pick_line_id, shares);
      }
      return new Map(
        [...byLine].map(([line, shares]) => [
          line,
          [...shares].map(([bl_order_id, qty]) => ({ bl_order_id, qty })),
        ]),
      );
    },
  });
}

/** Sets or clears one line's `picked_qty`, patching the cached run in place. */
export function useSetPicked(runId) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, picked }) => loco.data('pick_line').update(id, { picked_qty: picked }),
    onSuccess: (record) =>
      qc.setQueryData(['pick_line', runId], (lines) =>
        lines?.map((l) => (l.id === record.id ? record : l)),
      ),
  });
}

export function useMarkDone() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id) => loco.data('pull_run').update(id, { status: 'done' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['pull_run'] }),
  });
}
