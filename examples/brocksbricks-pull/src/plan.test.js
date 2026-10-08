import assert from 'node:assert/strict';
import { test } from 'node:test';
import { allocationFields, buildPlan, pickLineFields } from './plan.js';

// Records in the shape `loco/bricklink.store:orders` / `:order_items` return
// them: `{ id, fields }`, the item id `{order_id}-{batch}-{inventory_id}`.
const orders = [{ id: '1001' }, { id: '1002' }, { id: '1003' }];

const item = (order, inv, fields) => ({
  id: `${order}-1-${inv ?? 'x9'}`,
  fields: {
    order_id: order,
    inventory_id: inv,
    item_type: 'PART',
    color_id: 5,
    color_name: 'Red',
    condition: 'N',
    ...fields,
  },
});

const items = [
  // One lot across two orders: 150 + 120 = 270, over the threshold.
  item('1001', '501', { item_no: '3005', qty: 150, remarks: 'A-01-1' }),
  item('1002', '501', { item_no: '3005', qty: 120, remarks: ' a-01-1 ' }),
  // Small lots, written out of walk order.
  item('1001', '502', { item_no: '3023', qty: 4, remarks: 'C-02-1' }),
  item('1002', '503', { item_no: '3001', qty: 2, remarks: 'B-07-3' }),
  // The same item in another color is another line.
  item('1002', '504', { item_no: '3023', qty: 3, color_id: 11, color_name: 'Black', remarks: 'C-02-1' }),
  // Remarks that match no location, and none at all.
  item('1001', '505', { item_no: '3010', qty: 6, remarks: 'see blue drawer' }),
  item('1002', null, { item_no: '3622', qty: 1, remarks: '' }),
  // Order 1003 is already in an open run.
  item('1003', '506', { item_no: '3005', qty: 500, remarks: 'A-01-1' }),
];

const locations = [
  { fields: { code: 'A-01-1', walk_order: 10 } },
  { fields: { code: 'B-07-3', walk_order: 20 } },
  { fields: { code: 'C-02-1', walk_order: 30 } },
];

const plan = buildPlan({ orders, items, locations, threshold: 200, allocated: ['1003'] });
const at = (station) => plan.lines.filter((l) => l.station === station);

test('skips orders already allocated in an open run', () => {
  assert.deepEqual(plan.included, ['1001', '1002']);
  assert.deepEqual(plan.skipped, ['1003']);
  assert.ok(plan.lines.every((l) => l.allocations.every((a) => a.bl_order_id !== '1003')));
});

test('a lot totalling at or above the threshold goes to the scale with its per-order split', () => {
  const [line, ...rest] = at('scale');
  assert.equal(rest.length, 0);
  assert.equal(line.item_no, '3005');
  assert.equal(line.location_code, 'A-01-1');
  assert.equal(line.walk_order, 10);
  assert.equal(line.qty, 270);
  assert.deepEqual(
    line.allocations.map((a) => [a.bl_order_id, a.order_item_id, a.qty]),
    [
      ['1001', '1001-1-501', 150],
      ['1002', '1002-1-501', 120],
    ],
  );
});

test('small lots go to the hand route in walk order', () => {
  assert.deepEqual(
    at('hand').map((l) => [l.walk_order, l.item_no, l.color_id, l.qty]),
    [
      [20, '3001', 5, 2],
      [30, '3023', 5, 4],
      [30, '3023', 11, 3],
    ],
  );
});

test('a remark that matches no location is listed as unlocated, never dropped', () => {
  const lines = at('unlocated');
  assert.deepEqual(
    lines.map((l) => [l.remarks, l.item_no, l.location_code, l.walk_order]),
    [
      ['', '3622', null, null],
      ['see blue drawer', '3010', null, null],
    ],
  );
  // Every input line of an included order is in exactly one allocation.
  const allocated = plan.lines.flatMap((l) => l.allocations.map((a) => a.order_item_id)).sort();
  const expected = items.filter((i) => i.fields.order_id !== '1003').map((i) => i.id).sort();
  assert.deepEqual(allocated, expected);
  assert.deepEqual(plan.count, { scale: 1, hand: 3, unlocated: 2 });
});

test('the threshold is inclusive', () => {
  const p = buildPlan({ orders, items, locations, threshold: 270, allocated: ['1003'] });
  assert.equal(p.lines.find((l) => l.item_no === '3005').station, 'scale');
});

test('stored fields omit what a line does not have', () => {
  const unlocated = at('unlocated')[0];
  assert.deepEqual(pickLineFields('run', unlocated), {
    pull_run_id: 'run',
    station: 'unlocated',
    item_no: '3622',
    item_type: 'PART',
    color_id: 5,
    color_name: 'Red',
    condition: 'N',
    qty: 1,
  });
  assert.deepEqual(allocationFields('run', 'line', unlocated.allocations[0]), {
    pull_run_id: 'run',
    pick_line_id: 'line',
    bl_order_id: '1002',
    order_item_id: '1002-1-x9',
    qty: 1,
  });
});

test('a line with no countable qty holds its whole order back, never as 0', () => {
  const extra = [
    item('1004', '601', { item_no: '3001', qty: 10, remarks: 'B-07-3' }),
    item('1004', '602', { item_no: '3002', remarks: 'B-07-3' }),
    item('1005', '603', { item_no: '3003', qty: 'lots', remarks: 'B-07-3' }),
    item('1006', '604', { item_no: '3004', qty: 0, remarks: 'B-07-3' }),
  ];
  const all = [...orders, { id: '1004' }, { id: '1005' }, { id: '1006' }];
  const p = buildPlan({ orders: all, items: [...items, ...extra], locations, threshold: 200, allocated: ['1003'] });
  assert.deepEqual(p.included, ['1001', '1002']);
  assert.deepEqual(p.held, [
    { bl_order_id: '1004', lines: [{ order_item_id: '1004-1-602', item_no: '3002', qty: null }] },
    { bl_order_id: '1005', lines: [{ order_item_id: '1005-1-603', item_no: '3003', qty: 'lots' }] },
    { bl_order_id: '1006', lines: [{ order_item_id: '1006-1-604', item_no: '3004', qty: 0 }] },
  ]);
  // None of a held order's lines is planned, including its good one.
  const planned = p.lines.flatMap((l) => l.allocations.map((a) => a.bl_order_id));
  assert.ok(!planned.some((id) => ['1004', '1005', '1006'].includes(id)));
  assert.deepEqual(p.count, plan.count);
});
