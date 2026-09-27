import assert from 'node:assert/strict';
import { test } from 'node:test';
import { LocoError, createClient } from './index.js';

/** A Storage stand-in. */
function memoryStorage() {
  const m = new Map();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
  };
}

/** A fetch that answers from `replies` in order and records each call. */
function fakeFetch(...replies) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url, ...init, body: init.body && JSON.parse(init.body) });
    const [status, body] = replies.shift();
    return { status, json: async () => body };
  };
  fn.calls = calls;
  return fn;
}

test('site headers are sent only when given', async () => {
  const fetch = fakeFetch([200, { ok: true, data: [] }], [200, { ok: true, data: [] }]);
  await createClient({ storage: memoryStorage(), fetch }).data('lot').list();
  await createClient({ projectId: 'a/b', siteId: 'dev', storage: memoryStorage(), fetch })
    .data('lot')
    .list();
  assert.deepEqual(fetch.calls[0].headers, {});
  assert.deepEqual(fetch.calls[1].headers, { 'X-Project-Id': 'a/b', 'X-Site-Id': 'dev' });
  assert.equal(fetch.calls[1].url, '/data/lot/list');
});

test('login stores the token per origin and sends it as Bearer', async () => {
  const storage = memoryStorage();
  const fetch = fakeFetch([200, { ok: true, data: { token: 't1' } }], [200, { ok: true, data: {} }]);
  const client = createClient({ origin: 'http://api', storage, fetch });
  const seen = [];
  client.onSessionChange((loggedIn, reason) => seen.push([loggedIn, reason]));

  await client.login('alice', 'password');
  await client.me();

  assert.equal(storage.getItem('loco_session:http://api'), 't1');
  assert.equal(fetch.calls[0].headers.Authorization, undefined);
  assert.equal(fetch.calls[1].url, 'http://api/auth/me');
  assert.equal(fetch.calls[1].headers.Authorization, 'Bearer t1');
  assert.deepEqual(seen, [[true, 'login']]);
  assert.equal(createClient({ storage, fetch }).isLoggedIn(), false, 'another origin, another session');
});

test('a 401 with a token drops the session and tells listeners', async () => {
  const storage = memoryStorage();
  storage.setItem('loco_session:same-origin', 'stale');
  const fetch = fakeFetch([401, { ok: false, error: 'session expired' }]);
  const client = createClient({ storage, fetch });
  const seen = [];
  client.onSessionChange((loggedIn, reason) => seen.push([loggedIn, reason]));

  await assert.rejects(client.me(), { name: 'LocoError', status: 401, message: 'session expired' });
  assert.equal(client.isLoggedIn(), false);
  assert.deepEqual(seen, [[false, 'expired']]);
});

test('a rejected write carries its diagnostics', async () => {
  const diagnostics = [
    { severity: 'error', kind: 'invalid_option', path: 'condition', message: 'condition: "mint" is not an option' },
  ];
  const fetch = fakeFetch([400, { ok: false, error: 'validation failed', diagnostics }]);
  const err = await createClient({ storage: memoryStorage(), fetch })
    .data('lot')
    .add({ condition: 'mint' })
    .catch((e) => e);
  assert.ok(err instanceof LocoError);
  assert.equal(err.status, 400);
  assert.deepEqual(err.diagnostics, diagnostics);
  assert.equal(err.message, diagnostics[0].message);
  assert.deepEqual(fetch.calls[0].body, { condition: 'mint' });
  assert.equal(fetch.calls[0].headers['Content-Type'], 'application/json');
});

test('queryAll follows cursors and keeps the caller limit', async () => {
  const fetch = fakeFetch(
    [200, { ok: true, data: { q: { records: [{ id: '1' }], cursor: 'c1' } } }],
    [200, { ok: true, data: { q: { records: [{ id: '2' }], cursor: null } } }],
  );
  const records = await createClient({ storage: memoryStorage(), fetch }).queryAll({
    collection: 'lot',
    limit: 1,
  });
  assert.deepEqual(records.map((r) => r.id), ['1', '2']);
  assert.deepEqual(fetch.calls[0].body, { queries: { q: { collection: 'lot', limit: 1, cursor: null } } });
  assert.equal(fetch.calls[1].body.queries.q.cursor, 'c1');
});

test('queryAll rejects with the failed query’s diagnostics', async () => {
  const diagnostics = [
    { severity: 'error', kind: 'unknown_field', path: 'q/where/field', message: 'unknown field `qtty` on lot' },
  ];
  const fetch = fakeFetch([200, { ok: true, data: { q: { error: 'query failed', diagnostics } } }]);
  await assert.rejects(
    createClient({ storage: memoryStorage(), fetch }).queryAll({ collection: 'lot' }),
    { name: 'LocoError', message: 'unknown field `qtty` on lot', diagnostics },
  );
});

test('an unreachable server is a LocoError with status 0', async () => {
  const fetch = async () => {
    throw new TypeError('Failed to fetch');
  };
  await assert.rejects(createClient({ storage: memoryStorage(), fetch }).me(), {
    name: 'LocoError',
    status: 0,
  });
});
