const BASE = '/api';

// The studio is itself a loco site — every authed request identifies the
// editor session via these headers (gates `/schema` + `/config` writes).
const STUDIO_PROJECT = 'loco/studio';
const STUDIO_SITE = 'studio';

function getSession() {
  return localStorage.getItem('loco_session');
}

export function setSession(token) {
  localStorage.setItem('loco_session', token);
}

export function clearSession() {
  localStorage.removeItem('loco_session');
}

export function isLoggedIn() {
  return !!getSession();
}

async function request(path, options = {}) {
  const headers = {
    'Content-Type': 'application/json',
    'X-Project-Id': STUDIO_PROJECT,
    'X-Site-Id': STUDIO_SITE,
    ...options.headers,
  };
  const session = getSession();
  if (session) headers['Authorization'] = `Bearer ${session}`;
  const res = await fetch(`${BASE}${path}`, { ...options, headers });
  const json = await res.json();
  if (!json.ok) throw new Error(json.error || 'Unknown error');
  return json.data;
}

const json = (method, body) => ({ method, body: JSON.stringify(body) });

// --- Auth ---

export async function login(username) {
  const data = await request('/auth/login', json('POST', { username }));
  setSession(data.token);
  return data;
}

export async function logout() {
  await request('/auth/logout', { method: 'POST' });
  clearSession();
}

export const getMe = () => request('/auth/me');

// --- Projects ---

export const listProjects = () =>
  request('/config/project/list');

export const getProject = (user, project) =>
  request(`/config/project/${user}/${project}`);

export const createProject = (body) =>
  request('/config/project', json('POST', body));

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

export const deleteDataset = (user, project, name) =>
  request(`/config/dataset/${user}/${project}/${name}`, { method: 'DELETE' });

// --- Schema (versioned) ---

export const listCollections = (user, project, version) =>
  request(`/schema/${user}/${project}/${version}/collection/list`);

export const listFields = (user, project, version, collection) =>
  request(`/schema/${user}/${project}/${version}/field/${collection}/list`);
