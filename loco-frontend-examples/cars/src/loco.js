const SITE = 'acme-cars';

async function request(path, options = {}) {
  const url = `/api${path}${path.includes('?') ? '&' : '?'}site=${SITE}`;
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  const json = await res.json();
  if (!json.ok) throw new Error(json.error || 'Unknown error');
  return json.data;
}

export function listRecords(user, project, collection) {
  return request(`/${user}/${project}/collection/${collection}/list`);
}

export function getRecord(user, project, collection, id) {
  return request(`/${user}/${project}/collection/${collection}/get/${id}`);
}

export function addRecord(user, project, collection, fields, owner = '') {
  return request(`/${user}/${project}/collection/${collection}/add`, {
    method: 'POST',
    body: JSON.stringify({ fields, owner }),
  });
}

export function deleteRecord(user, project, collection, id) {
  return request(`/${user}/${project}/collection/${collection}/delete/${id}`, {
    method: 'DELETE',
  });
}

export function getSchemaCollections() {
  return request('/schema/collections');
}
