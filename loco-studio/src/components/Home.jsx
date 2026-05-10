import { Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listProjects, createProject } from '../api.js';

export default function Home() {
  const qc = useQueryClient();
  const { data: projects, isLoading, error } = useQuery({
    queryKey: ['projects'],
    queryFn: listProjects,
  });

  const addProject = useMutation({
    mutationFn: createProject,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['projects'] }),
  });

  const handleAdd = (e) => {
    e.preventDefault();
    const form = e.target;
    addProject.mutate(
      {
        name: form.elements.name.value,
        label: form.elements.label.value,
        description: form.elements.description.value,
      },
      { onSuccess: () => form.reset() },
    );
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <section>
      <div className="section-header">
        <h2>Projects <span className="count">({projects.length})</span></h2>
      </div>
      <form className="add-form" onSubmit={handleAdd}>
        <input name="label" placeholder="Project label" required />
        <input name="name" placeholder="Project name (e.g. crm)" required />
        <input name="description" placeholder="Description" />
        <button type="submit" disabled={addProject.isPending}>Create Project</button>
      </form>
      {addProject.error && <p className="error">{addProject.error.message}</p>}
      <div className="projects-grid">
        {projects.length === 0 && <p className="empty-state">No projects yet.</p>}
        {projects.map(([, fields]) => {
          const [user, project] = fields.project.split('/');
          return (
            <Link
              key={fields.project}
              to={`/projects/${user}/${project}`}
              className="project-card"
            >
              <h3>{fields.label || 'Unnamed'}</h3>
              <p className="project-ns">{fields.project}</p>
              <p className="project-desc">{fields.description || ''}</p>
            </Link>
          );
        })}
      </div>
    </section>
  );
}
