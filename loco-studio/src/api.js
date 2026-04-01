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

// Studio data lives in the "studio" dataset via the ?site=studio param
const STUDIO_SITE = 'site=studio';

export async function listProjects() {
  return request(`/loco/studio/collection/project/list?${STUDIO_SITE}`);
}

export async function addProject(fields) {
  return request(`/loco/studio/collection/project/add?${STUDIO_SITE}`, {
    method: 'POST',
    body: JSON.stringify({ fields }),
  });
}

export async function deleteProject(id) {
  return request(`/loco/studio/collection/project/delete/${id}?${STUDIO_SITE}`, {
    method: 'DELETE',
  });
}

export async function getProject(id) {
  return request(`/loco/studio/collection/project/get/${id}?${STUDIO_SITE}`);
}

export async function listSites() {
  return request(`/loco/studio/collection/site/list?${STUDIO_SITE}`);
}

export async function addSite(fields) {
  return request(`/loco/studio/collection/site/add?${STUDIO_SITE}`, {
    method: 'POST',
    body: JSON.stringify({ fields }),
  });
}

export async function getSite(id) {
  return request(`/loco/studio/collection/site/get/${id}?${STUDIO_SITE}`);
}

export async function updateSite(id, fields) {
  return request(`/loco/studio/collection/site/update/${id}?${STUDIO_SITE}`, {
    method: 'PUT',
    body: JSON.stringify({ fields }),
  });
}

export async function deleteSite(id) {
  return request(`/loco/studio/collection/site/delete/${id}?${STUDIO_SITE}`, {
    method: 'DELETE',
  });
}

export async function listDatasets() {
  return request(`/loco/studio/collection/dataset/list?${STUDIO_SITE}`);
}

export async function addDataset(fields) {
  return request(`/loco/studio/collection/dataset/add?${STUDIO_SITE}`, {
    method: 'POST',
    body: JSON.stringify({ fields }),
  });
}

export async function getDataset(id) {
  return request(`/loco/studio/collection/dataset/get/${id}?${STUDIO_SITE}`);
}

export async function deleteDataset(id) {
  return request(`/loco/studio/collection/dataset/delete/${id}?${STUDIO_SITE}`, {
    method: 'DELETE',
  });
}
