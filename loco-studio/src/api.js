import { API_ORIGIN } from './config.js';

/** In-memory API origin for this page load. `''` means same-origin. */
let apiOrigin = '';

function normalizeApiOrigin(raw) {
  if (raw == null) return null;
  const trimmed = String(raw).trim();
  if (trimmed === '') return '';
  let u;
  try {
    u = new URL(trimmed);
  } catch {
    return null;
  }
  if (u.username || u.password) return null;
  if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
  let path = u.pathname;
  if (path.endsWith('/') && path.length > 1) path = path.slice(0, -1);
  if (path === '' || path === '/') return u.origin;
  return `${u.origin}${path}`;
}

function canonicalizeSameOrigin(origin) {
  if (!origin) return '';
  let u;
  try {
    u = new URL(origin);
  } catch {
    return origin;
  }
  const pathOk = u.pathname === '/' || u.pathname === '';
  if (u.origin === window.location.origin && pathOk && !u.search && !u.hash) {
    return '';
  }
  return origin;
}

function applyOrigin(raw) {
  const normalized = normalizeApiOrigin(raw);
  if (normalized === null) {
    console.error('Invalid API origin', raw);
    apiOrigin = '';
    return;
  }
  apiOrigin = canonicalizeSameOrigin(normalized);
}

applyOrigin(API_ORIGIN);

function apiUrl(path) {
  return `${apiOrigin}${path}`;
}

function sessionKey() {
  return `loco_session:${apiOrigin || 'same-origin'}`;
}

function getSession() {
  return localStorage.getItem(sessionKey());
}

export function setSession(token) {
  localStorage.setItem(sessionKey(), token);
}

export function clearSession() {
  localStorage.removeItem(sessionKey());
}

export function isLoggedIn() {
  return !!getSession();
}

function displayOrigin() {
  return apiOrigin || window.location.origin;
}

async function request(path, options = {}) {
  const headers = {
    'Content-Type': 'application/json',
    ...options.headers,
  };
  const session = getSession();
  if (session) headers['Authorization'] = `Bearer ${session}`;
  let res;
  try {
    res = await fetch(apiUrl(path), { ...options, headers });
  } catch {
    throw new Error(`Cannot reach API at ${displayOrigin()}`);
  }
  const json = await res.json().catch(() => {
    throw new Error(`API at ${displayOrigin()} returned non-JSON`);
  });
  if (res.status === 401 && session && path !== '/auth/login') {
    clearSession();
    if (window.location.hash !== '#/login') {
      window.location.hash = '#/login';
    }
  }
  if (!json.ok) throw new Error(json.error || 'Unknown error');
  return json.data;
}

const json = (method, body) => ({ method, body: JSON.stringify(body) });

// --- Auth ---

export async function login({ username, password }) {
  const data = await request('/auth/login', json('POST', { username, password }));
  setSession(data.token);
  return data;
}

export async function logout() {
  try {
    await request('/auth/logout', { method: 'POST' });
  } finally {
    clearSession();
  }
}

export const getMe = () => request('/auth/me');

// --- Projects ---

export const listProjects = () =>
  request('/config/project/list');

export const getProject = (user, project) =>
  request(`/config/project/${user}/${project}`);

export const createProject = (body) =>
  request('/config/project', json('POST', body));

export const updateProject = (user, project, patch) =>
  request(`/config/project/${user}/${project}`, json('PUT', patch));

export const deleteProject = (user, project) =>
  request(`/config/project/${user}/${project}`, { method: 'DELETE' });

// --- Sites ---

export const listSites = (user, project) =>
  request(`/config/site/${user}/${project}/list`);

export const getSite = (user, project, name) =>
  request(`/config/site/${user}/${project}/${name}`);

export const createSite = (user, project, body) =>
  request(`/config/site/${user}/${project}`, json('POST', body));

export const updateSite = (user, project, name, patch) =>
  request(`/config/site/${user}/${project}/${name}`, json('PUT', patch));

export const deleteSite = (user, project, name) =>
  request(`/config/site/${user}/${project}/${name}`, { method: 'DELETE' });

// --- Datasets ---

export const listDatasets = (user, project) =>
  request(`/config/dataset/${user}/${project}/list`);

export const getDataset = (user, project, name) =>
  request(`/config/dataset/${user}/${project}/${name}`);

export const createDataset = (user, project, body) =>
  request(`/config/dataset/${user}/${project}`, json('POST', body));

export const updateDataset = (user, project, name, patch) =>
  request(`/config/dataset/${user}/${project}/${name}`, json('PUT', patch));

export const deleteDataset = (user, project, name) =>
  request(`/config/dataset/${user}/${project}/${name}`, { method: 'DELETE' });

// --- Versions (a version is a manifest under a project) ---

export const listVersions = (user, project) =>
  request(`/config/version/${user}/${project}/list`);

export const createVersion = (user, project, body) =>
  request(`/config/version/${user}/${project}`, json('POST', body));

export const deleteVersion = (user, project, version) =>
  request(`/config/version/${user}/${project}/${version}`, { method: 'DELETE' });

export const getManifest = (user, project, version) =>
  request(`/schema/${user}/${project}/${version}/manifest`);

export const updateManifest = (user, project, version, patch) =>
  request(`/schema/${user}/${project}/${version}/manifest`, json('PUT', patch));

// --- Collections (within a version) ---

export const listCollections = (user, project, version) =>
  request(`/schema/${user}/${project}/${version}/collection/list`);

export const getCollection = (user, project, version, name) =>
  request(`/schema/${user}/${project}/${version}/collection/${name}`);

export const createCollection = (user, project, version, body) =>
  request(`/schema/${user}/${project}/${version}/collection`, json('POST', body));

export const updateCollection = (user, project, version, name, patch) =>
  request(`/schema/${user}/${project}/${version}/collection/${name}`, json('PUT', patch));

export const deleteCollection = (user, project, version, name) =>
  request(`/schema/${user}/${project}/${version}/collection/${name}`, { method: 'DELETE' });

// --- Fields (within a collection within a version) ---

export const listFields = (user, project, version, collection) =>
  request(`/schema/${user}/${project}/${version}/field/${collection}/list`);

export const createField = (user, project, version, body) =>
  request(`/schema/${user}/${project}/${version}/field`, json('POST', body));

export const updateField = (user, project, version, collection, name, patch) =>
  request(`/schema/${user}/${project}/${version}/field/${collection}/${name}`, json('PUT', patch));

export const deleteField = (user, project, version, collection, name) =>
  request(`/schema/${user}/${project}/${version}/field/${collection}/${name}`, { method: 'DELETE' });

// --- Fieldsets (ordered subsets of a collection's fields) ---

export const listFieldsets = (user, project, version, collection) =>
  request(`/schema/${user}/${project}/${version}/fieldset/${collection}/list`);

export const updateFieldset = (user, project, version, collection, name, patch) =>
  request(`/schema/${user}/${project}/${version}/fieldset/${collection}/${name}`, json('PUT', patch));

// --- Records (data) ---
//
// Data routes scope by request headers, not URL. So each call overrides
// X-Project-Id and X-Site-Id to target the (project, site) being browsed
// instead of the studio's own.

const siteHeaders = (projectId, siteName) => ({
  'X-Project-Id': projectId,
  'X-Site-Id': siteName,
});

export const listRecords = (projectId, siteName, collection) =>
  request(`/data/${collection}/list`, { headers: siteHeaders(projectId, siteName) });

export const getRecord = (projectId, siteName, collection, id) =>
  request(`/data/${collection}/get/${id}`, { headers: siteHeaders(projectId, siteName) });

export const addRecord = (projectId, siteName, collection, fields) =>
  request(`/data/${collection}/add`, {
    ...json('POST', fields),
    headers: siteHeaders(projectId, siteName),
  });

export const updateRecord = (projectId, siteName, collection, id, fields) =>
  request(`/data/${collection}/update/${id}`, {
    ...json('PUT', fields),
    headers: siteHeaders(projectId, siteName),
  });

export const deleteRecord = (projectId, siteName, collection, id) =>
  request(`/data/${collection}/delete/${id}`, {
    method: 'DELETE',
    headers: siteHeaders(projectId, siteName),
  });
