import { listRecords, addRecord, deleteRecord } from './loco.js';
import './style.css';

const collections = [
  { user: 'ben', project: 'cars', name: 'vehicle', fields: ['make', 'model', 'year'] },
  { user: 'ben', project: 'crm', name: 'account', fields: ['company', 'active'] },
  { user: 'ben', project: 'crm', name: 'contact', fields: ['first_name', 'last_name'] },
];

function renderApp() {
  document.querySelector('#app').innerHTML = `
    <h1>Loco Cars Demo</h1>
    <nav>${collections.map((c) => `<button data-collection="${c.name}">${c.name}</button>`).join(' ')}</nav>
    <div id="content"></div>
  `;

  document.querySelectorAll('nav button').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('nav button').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      const col = collections.find((c) => c.name === btn.dataset.collection);
      renderCollection(col);
    });
  });

  document.querySelector('nav button').click();
}

async function renderCollection(col) {
  const content = document.querySelector('#content');
  content.innerHTML = '<p>Loading...</p>';

  try {
    const records = await listRecords(col.user, col.project, col.name);
    content.innerHTML = `
      <h2>${col.name} <span class="count">(${records.length})</span></h2>
      <form id="add-form">
        ${col.fields
          .map((f) => `<input name="${f}" placeholder="${f}" required />`)
          .join('')}
        <button type="submit">Add</button>
      </form>
      <table>
        <thead>
          <tr>
            ${col.fields.map((f) => `<th>${f}</th>`).join('')}
            <th></th>
          </tr>
        </thead>
        <tbody>
          ${records
            .map(
              (r) => `
            <tr>
              ${col.fields.map((f) => `<td>${r.fields[f] ?? ''}</td>`).join('')}
              <td><button class="delete" data-id="${r.id}">delete</button></td>
            </tr>`
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
        let val = form.elements[f].value;
        if (f === 'year') val = parseInt(val, 10);
        else if (f === 'active') val = val.toLowerCase() === 'true';
        fields[f] = val;
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
