import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as api from './api.js';

export const useRecords = (collection) =>
  useQuery({ queryKey: [collection], queryFn: () => api.list(collection) });

// A batch's lots, filtered and ordered by the server.
const batchLots = (batchId, extra) =>
  api.queryAll({
    collection: 'lot',
    where: { field: 'batch_id', op: 'eq', value: batchId },
    ...extra,
  });

// The server compares strings as bytes (`10247` before `3001`). Re-sort by
// item_no numerically for display; the sort is stable, so the server's
// color and condition order still breaks ties.
const collator = new Intl.Collator(undefined, { numeric: true });
const byItemNo = (a, b) => collator.compare(String(a ?? ''), String(b ?? ''));

export function useLots(batchId) {
  // Under ['lot'], so a lot write, which invalidates ['lot'], refetches it.
  const q = useQuery({
    queryKey: ['lot', { batchId }],
    queryFn: () =>
      batchLots(batchId, {
        order: [{ field: 'item_no' }, { field: 'color_code' }, { field: 'condition' }],
      }).then((lots) => lots.sort((a, b) => byItemNo(a.fields.item_no, b.fields.item_no))),
  });
  return { ...q, data: q.data ?? [] };
}

/** A field's `options` ({ value, label }) from its /schema metadata. */
export function useOptions(collection, field) {
  const q = useQuery({ queryKey: ['fields', collection], queryFn: () => api.fields(collection) });
  return q.data?.find((f) => f.name === field)?.options ?? [];
}

/** code → label lookups from the catalog, for display next to raw codes. */
export function useCatalog() {
  const colors = useRecords('color');
  const items = useRecords('item');
  const colorLabel = new Map((colors.data ?? []).map((c) => [c.fields.code, c.fields.label]));
  const itemLabel = new Map((items.data ?? []).map((i) => [i.fields.item_no, i.fields.label]));
  return { colorLabel, itemLabel };
}

function useWrite(collection, fn) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => qc.invalidateQueries({ queryKey: [collection] }),
  });
}

export const useAdd = (collection) => useWrite(collection, (fields) => api.add(collection, fields));
export const useUpdate = (collection) =>
  useWrite(collection, ({ id, fields }) => api.update(collection, id, fields));
export const useRemove = (collection) => useWrite(collection, (id) => api.remove(collection, id));

/** Deletes a batch's lots, then the batch. There is no cascade on /data. */
export function useRemoveBatch() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (batchId) => {
      for (const lot of await batchLots(batchId, { fields: [] })) {
        await api.remove('lot', lot.id);
      }
      await api.remove('batch', batchId);
    },
    // `exact`: the deleted batch's own ['batch', id] query must not refetch
    // into a 404 before the page navigates away.
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['lot'] });
      qc.invalidateQueries({ queryKey: ['batch'], exact: true });
    },
  });
}
