const BASE = '/api';

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

export async function request(path, options = {}) {
  const headers = { 'Content-Type': 'application/json', ...options.headers };
  const session = getSession();
  if (session) {
    headers['Authorization'] = `Bearer ${session}`;
  }
  const res = await fetch(`${BASE}${path}`, { ...options, headers });
  const json = await res.json();
  if (!json.ok) throw new Error(json.error || 'Unknown error');
  return json.data;
}

export async function login(username) {
  const data = await request('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ username, site_id: 'studio' }),
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

export async function listCollections(user, project, version) {
  return request(`/schema/${user}/${project}/${version}/collection/list`);
}

export async function listFields(user, project, version, collection) {
  return request(`/schema/${user}/${project}/${version}/field/${collection}/list`);
}

export async function listAllMeta(user, project, typeName) {
  return request(`/meta/${user}/${project}/${typeName}/list`);
}

export async function getSiteCollections(siteId) {
  return request(`/schema/collections?site=${encodeURIComponent(siteId)}`);
}

// --- Config CRUD (projects, sites, datasets) ---

export async function listConfig(typeName) {
  return request(`/config/${typeName}/list`);
}

export async function getConfig(typeName, id) {
  return request(`/config/get/${typeName}/${id}`);
}

export async function createConfig(typeName, id, fields) {
  return request(`/config/create/${typeName}/${id}`, {
    method: 'POST',
    body: JSON.stringify({ fields }),
  });
}

export async function updateConfig(typeName, id, fields) {
  return request(`/config/update/${typeName}/${id}`, {
    method: 'PUT',
    body: JSON.stringify({ fields }),
  });
}

export async function deleteConfig(typeName, id) {
  return request(`/config/delete/${typeName}/${id}`, {
    method: 'DELETE',
  });
}

// --- Project helpers ---

export async function listProjects() {
  return listConfig('project');
}

export async function getProject(id) {
  return getConfig('project', id);
}

export async function addProject(fields) {
  const ns = fields.namespace;
  const id = `projects/${ns}/project`;
  return createConfig('project', id, fields);
}

export async function deleteProject(id) {
  return deleteConfig('project', id);
}

// --- Site helpers ---

export async function listSites() {
  return listConfig('site');
}

export async function getSite(id) {
  return getConfig('site', id);
}

export async function addSite(fields) {
  const ns = fields.namespace.split('@')[0]; // strip version
  const id = `projects/${ns}/sites/${fields.site_id}`;
  return createConfig('site', id, fields);
}

export async function updateSite(id, fields) {
  return updateConfig('site', id, fields);
}

export async function deleteSite(id) {
  return deleteConfig('site', id);
}

// --- Dataset helpers ---

export async function listDatasets() {
  return listConfig('dataset');
}

export async function getDataset(id) {
  return getConfig('dataset', id);
}

export async function addDataset(namespace, fields) {
  const id = `projects/${namespace}/datasets/${fields.dataset_id}`;
  return createConfig('dataset', id, fields);
}

export async function deleteDataset(id) {
  return deleteConfig('dataset', id);
}
