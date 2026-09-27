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

export function useLots(batchId) {
  // Under ['lot'], so a lot write, which invalidates ['lot'], refetches it.
  // `natural` puts item 3001 before 10247, as a person reads part numbers.
  const q = useQuery({
    queryKey: ['lot', { batchId }],
    queryFn: () =>
      batchLots(batchId, {
        order: [
          { field: 'item_no', collation: 'natural' },
          { field: 'color_code' },
          { field: 'condition' },
        ],
      }),
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
