import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { createClient } from 'loco-client';

// Hosted at dev.inventory.brickos.<host>, the server infers the site from the
// Host header. Under `vite dev` the page is on localhost:5177 and the proxy
// reaches the apex, which has no site, so the client names it.
export const loco = createClient(
  import.meta.env.DEV ? { projectId: 'brickos/inventory', siteId: 'dev' } : {},
);

export const useRecords = (collection) =>
  useQuery({ queryKey: [collection], queryFn: () => loco.data(collection).list() });

// A batch's lots, filtered and ordered by the server.
const batchLots = (batchId, extra) =>
  loco.queryAll({
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

/** A field's `options` ({ value, label }), from the version this site pins. */
export function useOptions(collection, field) {
  const q = useQuery({
    queryKey: ['fields', collection],
    queryFn: () => loco.data(collection).fields(),
  });
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

export const useAdd = (collection) =>
  useWrite(collection, (fields) => loco.data(collection).add(fields));
export const useUpdate = (collection) =>
  useWrite(collection, ({ id, fields }) => loco.data(collection).update(id, fields));
export const useRemove = (collection) =>
  useWrite(collection, (id) => loco.data(collection).remove(id));

/** Deletes a batch's lots, then the batch. There is no cascade on /data. */
export function useRemoveBatch() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (batchId) => {
      for (const lot of await batchLots(batchId, { fields: [] })) {
        await loco.data('lot').remove(lot.id);
      }
      await loco.data('batch').remove(batchId);
    },
    // `exact`: the deleted batch's own ['batch', id] query must not refetch
    // into a 404 before the page navigates away.
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['lot'] });
      qc.invalidateQueries({ queryKey: ['batch'], exact: true });
    },
  });
}
