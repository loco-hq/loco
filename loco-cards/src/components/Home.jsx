import { useState, useEffect, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { listProjects, createProject } from '../api.js';
import { useAuth } from './AuthContext.jsx';

export default function Home() {
  const { user } = useAuth();
  const [projects, setProjects] = useState(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [error, setError] = useState(null);

  const load = useCallback(async () => {
    try {
      setProjects(await listProjects());
    } catch (err) {
      setError(err.message);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleCreate = async (e) => {
    e.preventDefault();
    if (!newName.trim()) return;
    const slug = newName.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-');
    const username = user?.username || 'me';
    try {
      await createProject(newName.trim(), `${username}/${slug}`);
      setNewName('');
      setCreating(false);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const handleCancel = () => {
    setCreating(false);
    setNewName('');
  };

  if (error) return <p className="error-banner">Something went wrong: {error}</p>;
  if (!projects) return <div className="loading">Shuffling the deck...</div>;

  const username = user?.username || '';
  const myProjects = projects.filter(([, fields]) =>
    fields.namespace?.startsWith(`${username}/`)
  );

  return (
    <section className="home">
      <h2 className="home-title">Your Projects</h2>
      <div className="cards-grid">
        {myProjects.map(([id, fields]) => (
          <Link key={id} to={`/project/${id}`} className="card card-link">
            <div className="card-suit">&#9830;</div>
            <h3 className="card-name">{fields.name}</h3>
            <p className="card-ns">{fields.namespace}</p>
          </Link>
        ))}

        {creating ? (
          <form className="card card-new-form" onSubmit={handleCreate}>
            <input
              className="new-name-input"
              type="text"
              placeholder="Project name"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              autoFocus
            />
            <div className="new-card-actions">
              <button type="submit" className="btn-create">Create</button>
              <button type="button" className="btn-cancel" onClick={handleCancel}>Cancel</button>
            </div>
          </form>
        ) : (
          <button className="card card-add" onClick={() => setCreating(true)}>
            <span className="card-add-plus">+</span>
            <span className="card-add-label">New Project</span>
          </button>
        )}
      </div>
    </section>
  );
}
