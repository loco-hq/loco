import { listRecords, addRecord, deleteRecord, getSchemaCollections } from './loco.js';
import './style.css';

async function renderApp() {
  const app = document.querySelector('#app');
  app.innerHTML = '<p>Loading schema...</p>';

  let schema;
  try {
    schema = await getSchemaCollections();
  } catch (err) {
    app.innerHTML = `<p class="error">Error loading schema: ${err.message}</p>`;
    return;
  }

  // Flatten all collections from all namespaces, extracting user/project from namespace
  const collections = schema.flatMap((ns) => {
    const [user, project] = ns.namespace.split('/');
    return ns.collections.map((col) => ({
      user,
      project,
      name: col.name,
      label: col.fields.label || col.name,
      fields: col.collection_fields.map(([, f]) => ({
        name: f.name,
        type: f.type,
      })),
    }));
  });

  app.innerHTML = `
    <h1>Loco Cars Demo</h1>
    <nav>${collections.map((c) => `<button data-collection="${c.name}" data-user="${c.user}" data-project="${c.project}">${c.label}</button>`).join(' ')}</nav>
    <div id="content"></div>
  `;

  document.querySelectorAll('nav button').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('nav button').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      const col = collections.find(
        (c) => c.name === btn.dataset.collection && c.user === btn.dataset.user && c.project === btn.dataset.project,
      );
      renderCollection(col);
    });
  });

  if (collections.length > 0) {
    document.querySelector('nav button').click();
  }
}

async function renderCollection(col) {
  const content = document.querySelector('#content');
  content.innerHTML = '<p>Loading...</p>';

  try {
    const records = await listRecords(col.user, col.project, col.name);
    content.innerHTML = `
      <h2>${col.label} <span class="count">(${records.length})</span></h2>
      <form id="add-form">
        ${col.fields
          .map((f) => `<input name="${f.name}" placeholder="${f.name}" required />`)
          .join('')}
        <button type="submit">Add</button>
      </form>
      <table>
        <thead>
          <tr>
            ${col.fields.map((f) => `<th>${f.name}</th>`).join('')}
            <th></th>
          </tr>
        </thead>
        <tbody>
          ${records
            .map(
              (r) => `
            <tr>
              ${col.fields.map((f) => `<td>${r.fields[f.name] ?? ''}</td>`).join('')}
              <td><button class="delete" data-id="${r.id}">delete</button></td>
            </tr>`,
            )
            .join('')}
        </tbody>
      </table>
    `;

    document.querySelector('#add-form').addEventListener('submit', async (e) => {
      e.preventDefault();
      const form = e.target;
      const fields = {};
      col.fields.forEach((f) => {
        let val = form.elements[f.name].value;
        if (f.type === 'integer') val = parseInt(val, 10);
        else if (f.type === 'float') val = parseFloat(val);
        else if (f.type === 'boolean') val = val.toLowerCase() === 'true';
        fields[f.name] = val;
      });
      await addRecord(col.user, col.project, col.name, fields);
      renderCollection(col);
    });

    content.querySelectorAll('.delete').forEach((btn) => {
      btn.addEventListener('click', async () => {
        await deleteRecord(col.user, col.project, col.name, btn.dataset.id);
        renderCollection(col);
      });
    });
  } catch (err) {
    content.innerHTML = `<p class="error">Error: ${err.message}</p>`;
  }
}

renderApp();
