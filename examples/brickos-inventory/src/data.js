import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as api from './api.js';

export const BATCH_KINDS = ['inventory', 'set', 'minifig', 'order', 'commission'];
export const CONDITIONS = ['new', 'used'];

// Whole-collection reads: /data has no filter yet (#15), so "lots in this
// batch" is every lot, filtered here.
export const useRecords = (collection) =>
  useQuery({ queryKey: [collection], queryFn: () => api.list(collection) });

export function useLots(batchId) {
  const q = useRecords('lot');
  const lots = (q.data ?? [])
    .filter((l) => l.fields.batch_id === batchId)
    .sort(
      (a, b) =>
        String(a.fields.item_no).localeCompare(String(b.fields.item_no), undefined, {
          numeric: true,
        }) ||
        (a.fields.color_code ?? 0) - (b.fields.color_code ?? 0) ||
        String(a.fields.condition).localeCompare(String(b.fields.condition)),
    );
  return { ...q, data: lots };
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
      const lots = await api.list('lot');
      for (const lot of lots.filter((l) => l.fields.batch_id === batchId)) {
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
