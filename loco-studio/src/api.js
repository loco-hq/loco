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

// --- Auth ---

export async function login(username) {
  const data = await request('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ username }),
  });
  setSession(data.token);
  return data;
}

export async function logout() {
  await request('/auth/logout', { method: 'POST' });
  clearSession();
}

export async function getMe() {
  return request('/auth/me');
}

// --- ID parsing ---
//
// The studio routes still carry full instance ids in the URL ("ben/crm/project",
// "ben/crm/sites/acme"). The backend's REST routes take (user, project[, name])
// as separate path segments, so we split here. PR 2 will move this into the
// router params.

function parseProjectId(id) {
  // "ben/crm/project" → { user: "ben", project: "crm" }
  const parts = id.split('/');
  return { user: parts[0], project: parts[1] };
}

function parseChildId(id) {
  // "ben/crm/sites/acme" or "ben/crm/datasets/acme" → { user, project, name }
  const parts = id.split('/');
  return { user: parts[0], project: parts[1], name: parts.slice(3).join('/') };
}

// --- Projects ---

export async function listProjects() {
  return request('/config/project/list');
}

export async function getProject(id) {
  const { user, project } = parseProjectId(id);
  return request(`/config/project/${user}/${project}`);
}

export async function addProject({ project, label, description }) {
  // Form input is the full path ("ben/crm" or just "crm"); the backend takes a
  // single-segment name and infers user from the auth session.
  const name = project.includes('/') ? project.split('/').pop() : project;
  return request('/config/project', {
    method: 'POST',
    body: JSON.stringify({ name, label, description: description || '' }),
  });
}

export async function deleteProject(id) {
  const { user, project } = parseProjectId(id);
  return request(`/config/project/${user}/${project}`, { method: 'DELETE' });
}

// --- Sites ---

export async function listSitesForProject(user, project) {
  return request(`/config/site/${user}/${project}/list`);
}

export async function getSite(id) {
  const { user, project, name } = parseChildId(id);
  return request(`/config/site/${user}/${project}/${name}`);
}

export async function addSite({ project, name, label, version, dataset }) {
  const [user, projectName] = project.split('/');
  const body = {
    name,
    label,
    version: version || '0.0.1-dev',
    dataset: dataset || '',
  };
  return request(`/config/site/${user}/${projectName}`, {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export async function updateSite(id, patch) {
  const { user, project, name } = parseChildId(id);
  return request(`/config/site/${user}/${project}/${name}`, {
    method: 'PUT',
    body: JSON.stringify(patch),
  });
}

export async function deleteSite(id) {
  const { user, project, name } = parseChildId(id);
  return request(`/config/site/${user}/${project}/${name}`, { method: 'DELETE' });
}

// --- Datasets ---

export async function listDatasetsForProject(user, project) {
  return request(`/config/dataset/${user}/${project}/list`);
}

export async function getDataset(id) {
  const { user, project, name } = parseChildId(id);
  return request(`/config/dataset/${user}/${project}/${name}`);
}

export async function addDataset({ project, name, label, description }) {
  const [user, projectName] = project.split('/');
  return request(`/config/dataset/${user}/${projectName}`, {
    method: 'POST',
    body: JSON.stringify({ name, label, description: description || '' }),
  });
}

export async function deleteDataset(id) {
  const { user, project, name } = parseChildId(id);
  return request(`/config/dataset/${user}/${project}/${name}`, { method: 'DELETE' });
}

// --- Schema (versioned) ---

export async function listCollections(user, project, version) {
  return request(`/schema/${user}/${project}/${version}/collection/list`);
}

export async function listFields(user, project, version, collection) {
  return request(`/schema/${user}/${project}/${version}/field/${collection}/list`);
}
