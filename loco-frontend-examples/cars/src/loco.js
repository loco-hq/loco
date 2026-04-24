const SITE = "yolo-cars";

async function request(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const json = await res.json();
  if (!json.ok) throw new Error(json.error || "Unknown error");
  return json.data;
}

function dataHeaders(user, project) {
  return { "X-Project-Id": `${user}/${project}`, "X-Site-Id": SITE };
}

export function listRecords(user, project, collection) {
  return request(`/data/${collection}/list`, { headers: dataHeaders(user, project) });
}

export function getRecord(user, project, collection, id) {
  return request(`/data/${collection}/get/${id}`, { headers: dataHeaders(user, project) });
}

export function addRecord(user, project, collection, fields, owner = "") {
  return request(`/data/${collection}/add`, {
    method: "POST",
    headers: dataHeaders(user, project),
    body: JSON.stringify({ fields, owner }),
  });
}

export function deleteRecord(user, project, collection, id) {
  return request(`/data/${collection}/delete/${id}`, {
    method: "DELETE",
    headers: dataHeaders(user, project),
  });
}

export function getSchemaCollections() {
  return request("/schema/collections");
}
