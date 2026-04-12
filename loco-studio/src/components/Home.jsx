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
      project: form.elements.project.value,
      label: form.elements.label.value,
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
        <input name="label" placeholder="Project label" required />
        <input name="project" placeholder="Project path (e.g. ben/crm)" required />
        <input name="description" placeholder="Description" />
        <button type="submit">Create Project</button>
      </form>
      <div className="projects-grid">
        {projects.length === 0 && <p className="empty-state">No projects yet.</p>}
        {projects.map(([id, fields]) => (
          <Link key={id} to={`/project/${id}`} className="project-card">
            <h3>{fields.label || 'Unnamed'}</h3>
            <p className="project-ns">{fields.project || ''}</p>
            <p className="project-desc">{fields.description || ''}</p>
          </Link>
        ))}
      </div>
    </section>
  );
}
