import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { getProject, deleteProject, listDecks, createDeck, deleteDeck } from '../api.js';

export default function ProjectDetail() {
  const { '*': projectId } = useParams();
  const navigate = useNavigate();
  const [project, setProject] = useState(null);
  const [decks, setDecks] = useState([]);
  const [error, setError] = useState(null);
  const [confirming, setConfirming] = useState(false);
  const [creatingDeck, setCreatingDeck] = useState(false);
  const [newDeckName, setNewDeckName] = useState('');

  // Parse project path into user/project and hardcode dev version
  const parseProjectPath = (path) => {
    const [user, proj] = (path || '').split('/');
    return { user, project: proj, version: '0.0.1-dev' };
  };

  const load = useCallback(async () => {
    try {
      const proj = await getProject(projectId);
      setProject(proj);
      const { user, project: projSlug, version } = parseProjectPath(proj.project);
      if (user && projSlug) {
        setDecks(await listDecks(user, projSlug, version));
      }
    } catch (err) {
      setError(err.message);
    }
  }, [projectId]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async () => {
    try {
      await deleteProject(projectId);
      navigate('/');
    } catch (err) {
      setError(err.message);
    }
  };

  const handleCreateDeck = async (e) => {
    e.preventDefault();
    if (!newDeckName.trim() || !project) return;
    const slug = newDeckName.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-');
    const { user, project: projSlug, version } = parseProjectPath(project.project);
    try {
      await createDeck(user, projSlug, version, slug);
      setNewDeckName('');
      setCreatingDeck(false);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  const handleDeleteDeck = async (name) => {
    const { user, project: projSlug, version } = parseProjectPath(project.project);
    try {
      await deleteDeck(user, projSlug, version, name);
      load();
    } catch (err) {
      setError(err.message);
    }
  };

  if (error) return <p className="error-banner">Something went wrong: {error}</p>;
  if (!project) return <div className="loading">Drawing a card...</div>;

  return (
    <section className="project-detail">
      <Link to="/" className="back-link">&#8592; All Projects</Link>

      <div className="detail-card">
        <div className="detail-suit">&#9830;</div>
        <h2 className="detail-name">{project.label}</h2>
        <p className="detail-ns">{project.project}</p>
        {project.description && (
          <p className="detail-desc">{project.description}</p>
        )}
      </div>

      <h3 className="decks-title">
        Decks <span className="count">({decks.length})</span>
      </h3>
      <div className="cards-grid">
        {decks.map(([ns, fields]) => {
          const name = ns.split('.').pop();
          return (
            <Link key={ns} to={`/deck/${projectId}/${name}`} className="card deck-card card-link">
              <div className="card-suit suit-spade">&#9824;</div>
              <h3 className="card-name">{fields.label || name}</h3>
              <p className="card-ns">{name}</p>
              <button
                className="deck-delete-btn"
                onClick={(e) => { e.preventDefault(); handleDeleteDeck(name); }}
                title="Delete deck"
              >
                &times;
              </button>
            </Link>
          );
        })}

        {creatingDeck ? (
          <form className="card card-new-form" onSubmit={handleCreateDeck}>
            <input
              className="new-name-input"
              type="text"
              placeholder="Deck name"
              value={newDeckName}
              onChange={(e) => setNewDeckName(e.target.value)}
              autoFocus
            />
            <div className="new-card-actions">
              <button type="submit" className="btn-create">Create</button>
              <button type="button" className="btn-cancel" onClick={() => { setCreatingDeck(false); setNewDeckName(''); }}>Cancel</button>
            </div>
          </form>
        ) : (
          <button className="card card-add" onClick={() => setCreatingDeck(true)}>
            <span className="card-add-plus">+</span>
            <span className="card-add-label">New Deck</span>
          </button>
        )}
      </div>

      <div className="detail-actions">
        {confirming ? (
          <div className="confirm-delete">
            <p>Delete this project and all its data?</p>
            <div className="confirm-buttons">
              <button className="btn-danger" onClick={handleDelete}>Yes, delete it</button>
              <button className="btn-cancel" onClick={() => setConfirming(false)}>Cancel</button>
            </div>
          </div>
        ) : (
          <button className="btn-delete" onClick={() => setConfirming(true)}>
            Delete Project
          </button>
        )}
      </div>
    </section>
  );
}
