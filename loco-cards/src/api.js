const BASE = '/api';

function getSession() {
  return localStorage.getItem('loco_cards_session');
}

function setSession(token) {
  localStorage.setItem('loco_cards_session', token);
}

function clearSession() {
  localStorage.removeItem('loco_cards_session');
}

export function isLoggedIn() {
  return !!getSession();
}

async function request(path, options = {}) {
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
    body: JSON.stringify({ username, site_id: 'cards' }),
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

export async function listProjects() {
  return request('/config/project/list');
}

export async function createProject(label, project) {
  const id = `projects/${project}/project`;
  return request(`/config/create/project/${id}`, {
    method: 'POST',
    body: JSON.stringify({
      fields: { label, description: '' },
    }),
  });
}

export async function getProject(id) {
  return request(`/config/get/project/${id}`);
}

export async function deleteProject(id) {
  return request(`/config/delete/project/${id}`, { method: 'DELETE' });
}

// --- Decks (collections) ---

export async function listDecks(user, project, version) {
  return request(`/schema/${user}/${project}/${version}/collection/list`);
}

export async function createDeck(user, project, version, name) {
  return request(`/schema/${user}/${project}/${version}/collection`, {
    method: 'POST',
    body: JSON.stringify({ name, label: name, label_plural: name }),
  });
}

export async function deleteDeck(user, project, version, name) {
  return request(`/schema/${user}/${project}/${version}/collection/${name}`, {
    method: 'DELETE',
  });
}

// --- Fields (properties on a deck) ---

export async function listFields(user, project, version, collection) {
  return request(`/schema/${user}/${project}/${version}/field/${collection}/list`);
}

export async function createField(user, project, version, collection, name, type) {
  return request(`/schema/${user}/${project}/${version}/field/${collection}`, {
    method: 'POST',
    body: JSON.stringify({ name, type }),
  });
}

export async function deleteField(user, project, version, collection, name) {
  return request(`/schema/${user}/${project}/${version}/field/${collection}/${name}`, {
    method: 'DELETE',
  });
}

// --- Records (cards in a deck) ---

export async function listRecords(user, project, collection, siteId) {
  return request(`/${user}/${project}/collection/${collection}/list?site=${encodeURIComponent(siteId)}`);
}

export async function addRecord(user, project, collection, siteId, fields) {
  return request(`/${user}/${project}/collection/${collection}/add?site=${encodeURIComponent(siteId)}`, {
    method: 'POST',
    body: JSON.stringify({ fields }),
  });
}

export async function updateRecord(user, project, collection, siteId, id, fields) {
  return request(`/${user}/${project}/collection/${collection}/update/${id}?site=${encodeURIComponent(siteId)}`, {
    method: 'PUT',
    body: JSON.stringify({ fields }),
  });
}

export async function deleteRecord(user, project, collection, siteId, id) {
  return request(`/${user}/${project}/collection/${collection}/delete/${id}?site=${encodeURIComponent(siteId)}`, {
    method: 'DELETE',
  });
}
