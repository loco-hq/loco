// Same-origin API client. Hosted at dev.inventory.brickos.<host>, the server
// infers the site from the Host header. Under `vite dev` the page is on
// localhost:5177 and the proxy reaches the apex, which has no site, so the
// headers are sent explicitly.
const SITE_HEADERS = import.meta.env.DEV
  ? { 'X-Project-Id': 'brickos/inventory', 'X-Site-Id': 'dev' }
  : {};

const SESSION_KEY = 'brickos_session';

export const isLoggedIn = () => !!localStorage.getItem(SESSION_KEY);

async function request(path, { method = 'GET', body } = {}) {
  const headers = { ...SITE_HEADERS };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const token = localStorage.getItem(SESSION_KEY);
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const res = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({ ok: false, error: `HTTP ${res.status}` }));
  if (res.status === 401 && token) {
    // Session expired or revoked: drop it and let the app show the login form.
    localStorage.removeItem(SESSION_KEY);
    window.dispatchEvent(new Event('brickos:logout'));
  }
  if (!json.ok) throw new Error(errorMessage(json));
  return json.data;
}

// A write that fails validation answers with per-field diagnostics.
function errorMessage(json) {
  if (json.diagnostics?.length) return json.diagnostics.map((d) => d.message).join('; ');
  return json.error || 'Unknown error';
}

// --- Auth ---

export async function login(username, password) {
  const data = await request('/auth/login', { method: 'POST', body: { username, password } });
  localStorage.setItem(SESSION_KEY, data.token);
  return data;
}

export async function logout() {
  try {
    await request('/auth/logout', { method: 'POST' });
  } finally {
    localStorage.removeItem(SESSION_KEY);
  }
}

export const getMe = () => request('/auth/me');

// --- Records ---

export const list = (collection) => request(`/data/${collection}/list`);
export const get = (collection, id) => request(`/data/${collection}/get/${id}`);
export const add = (collection, fields) =>
  request(`/data/${collection}/add`, { method: 'POST', body: fields });
export const update = (collection, id, fields) =>
  request(`/data/${collection}/update/${id}`, { method: 'PUT', body: fields });
export const remove = (collection, id) =>
  request(`/data/${collection}/delete/${id}`, { method: 'DELETE' });
