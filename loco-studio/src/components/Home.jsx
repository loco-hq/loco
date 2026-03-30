import { useState, useEffect, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { listProjects, addProject } from '../api.js';

export default function Home() {
  const [projects, setProjects] = useState(null);
  const [error, setError] = useState(null);

  const load = useCallback(async () => {
    try {
      setProjects(await listProjects());
    } catch (err) {
      setError(err.message);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleAdd = async (e) => {
    e.preventDefault();
    const form = e.target;
    await addProject({
      name: form.elements.name.value,
      namespace: form.elements.namespace.value,
      description: form.elements.description.value,
    });
    form.reset();
    load();
  };

  if (error) return <p className="error">Error: {error}</p>;
  if (!projects) return <p>Loading...</p>;

  return (
    <section>
      <div className="section-header">
        <h2>Projects <span className="count">({projects.length})</span></h2>
      </div>
      <form className="add-form" onSubmit={handleAdd}>
        <input name="name" placeholder="Project name" required />
        <input name="namespace" placeholder="Namespace (e.g. ben/crm)" required />
        <input name="description" placeholder="Description" />
        <button type="submit">Create Project</button>
      </form>
      <div className="projects-grid">
        {projects.length === 0 && <p className="empty-state">No projects yet.</p>}
        {projects.map((p) => (
          <Link key={p.id} to={`/project/${p.id}`} className="project-card">
            <h3>{p.fields.name || 'Unnamed'}</h3>
            <p className="project-ns">{p.fields.namespace || ''}</p>
            <p className="project-desc">{p.fields.description || ''}</p>
          </Link>
        ))}
      </div>
    </section>
  );
}
