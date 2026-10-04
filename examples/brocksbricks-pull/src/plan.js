// Building a pull plan from BrickLink orders and their lines. Pure: no I/O,
// no React, so `node --test` runs it on fixture records (plan.test.js).

/** Stations, in the order a run lists them. */
export const STATIONS = ['scale', 'hand', 'unlocated'];

/**
 * A remark's lookup key. v1 is a lookup, not a parser: the trimmed remark,
 * case-folded, must equal a `location.code` the same way. Whatever the store
 * writes in remarks is configured as `location` records, not in code.
 * @param {unknown} text
 */
export const locationKey = (text) => String(text ?? '').trim().toLowerCase();

/**
 * Groups the lines of the orders not already in an open run into pick lines.
 *
 * A located line is keyed by item, type, color, condition, and location, and
 * summed across orders. Its total decides the station: at or above
 * `threshold` is `scale`, below is `hand`. A line whose remark matches no
 * location, or is empty, is `unlocated`: keyed by its trimmed remark instead,
 * so two different notes never merge, and kept, never dropped.
 *
 * Lines come back in walk order within each station, then by item number
 * (natural), color, and condition. Unlocated lines have no walk order and
 * sort by remark, then item.
 *
 * @param {object} input
 * @param {Array<{ id: string }>} input.orders The `orders` records read for this run.
 * @param {Array<{ id: string, fields: Record<string, unknown> }>} input.items
 *   `order_items` records of those orders.
 * @param {Array<{ fields: { code: string, walk_order: number, zone?: string } }>} input.locations
 * @param {number} input.threshold `settings.scale_threshold`.
 * @param {Iterable<string>} [input.allocated] Order ids already allocated in an open run.
 */
export function buildPlan({ orders, items, locations, threshold, allocated = [] }) {
  const taken = new Set(allocated);
  const included = orders.map((o) => o.id).filter((id) => !taken.has(id));
  const skipped = orders.map((o) => o.id).filter((id) => taken.has(id));
  const wanted = new Set(included);

  const byCode = new Map();
  for (const loc of locations) {
    const key = locationKey(loc.fields.code);
    if (key && !byCode.has(key)) byCode.set(key, loc.fields);
  }

  const groups = new Map();
  for (const item of items) {
    const f = item.fields;
    const orderId = String(f.order_id);
    if (!wanted.has(orderId)) continue;
    const remarks = String(f.remarks ?? '').trim();
    const loc = byCode.get(locationKey(remarks));
    const where = loc ? `@${locationKey(loc.code)}` : `?${remarks}`;
    const key = [f.item_type, f.item_no, f.color_id, f.condition, where].join('\u0000');
    let line = groups.get(key);
    if (!line) {
      line = {
        key,
        location_code: loc ? loc.code : null,
        walk_order: loc ? loc.walk_order : null,
        item_no: f.item_no,
        item_type: f.item_type,
        color_id: f.color_id,
        color_name: f.color_name ?? null,
        condition: f.condition,
        remarks,
        qty: 0,
        allocations: [],
      };
      groups.set(key, line);
    }
    const qty = Number(f.qty) || 0;
    line.qty += qty;
    line.allocations.push({
      bl_order_id: orderId,
      order_item_id: item.id,
      inventory_id: f.inventory_id == null ? null : String(f.inventory_id),
      qty,
    });
  }

  const lines = [...groups.values()].map((line) => ({
    ...line,
    station: line.location_code == null ? 'unlocated' : line.qty >= threshold ? 'scale' : 'hand',
  }));
  lines.sort(compareLines);

  const count = Object.fromEntries(STATIONS.map((s) => [s, 0]));
  for (const line of lines) count[line.station] += 1;
  return { included, skipped, lines, count };
}

const natural = new Intl.Collator('en', { numeric: true });

/** Station, then walk order (unlocated: remark), then item, color, condition. */
export function compareLines(a, b) {
  // Within one station either both lines have a walk order or neither does.
  return (
    STATIONS.indexOf(a.station) - STATIONS.indexOf(b.station) ||
    (a.walk_order ?? 0) - (b.walk_order ?? 0) ||
    natural.compare(a.walk_order == null ? a.remarks ?? '' : '', b.walk_order == null ? b.remarks ?? '' : '') ||
    natural.compare(String(a.item_no), String(b.item_no)) ||
    (a.color_id ?? 0) - (b.color_id ?? 0) ||
    String(a.condition).localeCompare(String(b.condition))
  );
}

/** A pick line's fields as `pick_line` stores them. */
export function pickLineFields(runId, line) {
  const fields = {
    pull_run_id: runId,
    station: line.station,
    item_no: line.item_no,
    item_type: line.item_type,
    color_id: line.color_id,
    condition: line.condition,
    qty: line.qty,
  };
  if (line.walk_order != null) fields.walk_order = line.walk_order;
  if (line.location_code != null) fields.location_code = line.location_code;
  if (line.color_name != null) fields.color_name = line.color_name;
  if (line.remarks) fields.remarks = line.remarks;
  return fields;
}

/** One allocation's fields as `pick_allocation` stores them. */
export function allocationFields(runId, lineId, alloc) {
  const fields = {
    pull_run_id: runId,
    pick_line_id: lineId,
    bl_order_id: alloc.bl_order_id,
    order_item_id: alloc.order_item_id,
    qty: alloc.qty,
  };
  if (alloc.inventory_id != null) fields.inventory_id = alloc.inventory_id;
  return fields;
}
