// A Loco API client: the session, the Bearer header, site headers, and the
// `{ ok, data, error, diagnostics }` envelope, in one place. Plain JS, no
// framework; an app wraps these calls in whatever data layer it uses.

/**
 * One problem the server found, from `validation.rs` or `/data/query`.
 * @typedef {{ severity: 'error' | 'warning', kind: string, path: string, message: string }} Diagnostic
 */

/**
 * A lake record. `fields` holds the collection's fields; the rest is metadata.
 * @typedef {{ id: string, created_at: string, created_by: string | null, updated_at: string,
 *   updated_by: string | null, owner: string | null, fields: Record<string, unknown> }} LocoRecord
 */

/** Anything the API answered with `ok: false`, or could not answer at all. */
export class LocoError extends Error {
  /**
   * @param {string} message
   * @param {{ status?: number, diagnostics?: Diagnostic[] }} [details]
   */
  constructor(message, { status = 0, diagnostics = [] } = {}) {
    super(message);
    this.name = 'LocoError';
    /** HTTP status; 0 when the request never got an answer. */
    this.status = status;
    /** Per-field problems when the server rejected a write or a query. */
    this.diagnostics = diagnostics;
  }
}

/**
 * `diagnostics` read better than the bare `error`, which for a rejected
 * write is only "validation failed".
 * @param {{ error?: string, diagnostics?: Diagnostic[] }} body
 */
function messageOf(body) {
  const errors = (body.diagnostics ?? []).filter((d) => d.severity !== 'warning');
  if (errors.length) return errors.map((d) => d.message).join('; ');
  return body.error || 'Unknown error';
}

/**
 * @typedef {object} ClientOptions
 * @property {string} [origin] API origin, e.g. `http://localhost:3000`. `''`
 *   (the default) is same-origin: a hosted bundle, or a dev proxy.
 * @property {string} [projectId] `X-Project-Id`, as `{account}/{project}`.
 * @property {string} [siteId] `X-Site-Id`. Send both only when the host name
 *   does not name the site already (docs/hosting.md); a hosted bundle omits them.
 * @property {Storage} [storage] Where the session token lives. Default `localStorage`.
 * @property {typeof fetch} [fetch] Default the global `fetch`, looked up per call so a
 *   wrapper installed later (a test shim, an interceptor) still sees requests.
 */

/**
 * @typedef {'login' | 'logout' | 'expired'} SessionReason
 * `expired` is a 401 on a request that carried a token: the session was
 * revoked or ran out, and the client has already dropped it.
 */

/** @param {ClientOptions} [options] */
export function createClient({
  origin = '',
  projectId,
  siteId,
  storage = globalThis.localStorage,
  fetch = (...args) => globalThis.fetch(...args),
} = {}) {
  // One session per API origin, so two clients on one page that talk to
  // different servers do not share a token.
  const sessionKey = `loco_session:${origin || 'same-origin'}`;
  const siteHeaders = {};
  if (projectId) siteHeaders['X-Project-Id'] = projectId;
  if (siteId) siteHeaders['X-Site-Id'] = siteId;

  /** @type {Set<(loggedIn: boolean, reason: SessionReason) => void>} */
  const listeners = new Set();

  const token = () => storage.getItem(sessionKey);

  /** @param {string | null} next @param {SessionReason} reason */
  function setToken(next, reason) {
    if (next) storage.setItem(sessionKey, next);
    else storage.removeItem(sessionKey);
    for (const listener of listeners) listener(!!next, reason);
  }

  /**
   * One API call. Resolves to the envelope's `data`; rejects with a `LocoError`.
   * @param {string} path
   * @param {{ method?: string, body?: unknown, auth?: boolean }} [init]
   */
  async function request(path, { method = 'GET', body, auth = true } = {}) {
    const headers = { ...siteHeaders };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    const sent = auth ? token() : null;
    if (sent) headers['Authorization'] = `Bearer ${sent}`;

    let res;
    try {
      res = await fetch(`${origin}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      throw new LocoError(`Cannot reach the API at ${origin || 'this origin'}`);
    }
    const json = await res
      .json()
      .catch(() => ({ ok: false, error: `HTTP ${res.status}, not JSON` }));
    if (res.status === 401 && sent && sent === token()) setToken(null, 'expired');
    if (!json.ok) {
      throw new LocoError(messageOf(json), { status: res.status, diagnostics: json.diagnostics });
    }
    return json.data;
  }

  /**
   * Record CRUD on one collection of the site's pinned version. A bare name is
   * the site's own collection; a dependency's is written qualified,
   * `'acme/crm.contacts'`, and sent as one encoded path segment.
   * @param {string} collection
   */
  function data(collection) {
    const base = `/data/${encodeURIComponent(collection)}`;
    const id = (value) => encodeURIComponent(value);
    return {
      /** Every record. For a filtered or paged read, use `query`. @returns {Promise<LocoRecord[]>} */
      list: () => request(`${base}/list`),
      /** @param {string} recordId @returns {Promise<LocoRecord>} */
      get: (recordId) => request(`${base}/get/${id(recordId)}`),
      /** @param {Record<string, unknown>} fields @returns {Promise<LocoRecord>} */
      add: (fields) => request(`${base}/add`, { method: 'POST', body: fields }),
      /** Merges `fields` into the record. @param {string} recordId @param {Record<string, unknown>} fields @returns {Promise<LocoRecord>} */
      update: (recordId, fields) =>
        request(`${base}/update/${id(recordId)}`, { method: 'PUT', body: fields }),
      /** @param {string} recordId */
      remove: (recordId) => request(`${base}/delete/${id(recordId)}`, { method: 'DELETE' }),
      /**
       * The collection's fields in the version the site pins (label, type,
       * `options`, …), in fieldset order. Follows a re-pin with no client change.
       * @returns {Promise<Array<{ name: string, type: string, label?: string,
       *   options?: Array<{ value: string, label: string }> } & Record<string, unknown>>>}
       */
      fields: () => request(`${base}/fields`),
    };
  }

  /**
   * One `POST /data/query` batch (docs/query.md). Resolves to the results by
   * query name; a query that failed is `{ error, diagnostics }` in its slot,
   * not a rejection. Use `queryAll` to page one query to the end.
   * @param {Record<string, object>} queries
   * @returns {Promise<Record<string, { records: LocoRecord[], cursor: string | null }
   *   | { error: string, diagnostics: Diagnostic[] }>>}
   */
  const query = (queries) => request('/data/query', { method: 'POST', body: { queries } });

  /**
   * Every record one query matches, following its cursor page by page.
   * Rejects with a `LocoError` carrying the query's diagnostics if it fails.
   * @param {object} q A single query: `{ collection, where?, order?, fields?, limit? }`.
   * @returns {Promise<LocoRecord[]>}
   */
  async function queryAll(q) {
    const records = [];
    let cursor = null;
    do {
      const { q: page } = await query({ q: { limit: 500, ...q, cursor } });
      if (page.error) throw new LocoError(messageOf(page), { status: 200, diagnostics: page.diagnostics });
      records.push(...page.records);
      cursor = page.cursor;
    } while (cursor);
    return records;
  }

  return {
    origin,
    isLoggedIn: () => !!token(),

    /**
     * Calls `listener(loggedIn, reason)` whenever the session starts or ends,
     * including a 401 that drops it. Returns an unsubscribe function, so it
     * fits React's `useSyncExternalStore(client.onSessionChange, client.isLoggedIn)`.
     * @param {(loggedIn: boolean, reason: SessionReason) => void} listener
     */
    onSessionChange(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },

    /** @param {string} username @param {string} [password] */
    async login(username, password) {
      const session = await request('/auth/login', {
        method: 'POST',
        body: { username, password },
        auth: false,
      });
      setToken(session.token, 'login');
      return session;
    },

    /** Ends the session on the server, and locally even if the server call fails. */
    async logout() {
      try {
        if (token()) await request('/auth/logout', { method: 'POST' });
      } finally {
        if (token()) setToken(null, 'logout');
      }
    },

    /** The signed-in identity. */
    me: () => request('/auth/me'),

    data,
    query,
    queryAll,

    /** Escape hatch for routes without a helper: resolves to the envelope's `data`. */
    request,
  };
}
