import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { createSite, listDatasets, listVersions } from '../api.js';

export default function NewSite() {
  const { user, project } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [searchParams] = useSearchParams();
  const prefilledVersion = searchParams.get('version') || '';

  const { data: datasets = [] } = useQuery({
    queryKey: ['datasets', user, project],
    queryFn: () => listDatasets(user, project),
  });

  const { data: versions = [] } = useQuery({
    queryKey: ['versions', user, project],
    queryFn: () => listVersions(user, project),
  });

  const create = useMutation({
    mutationFn: (body) => createSite(user, project, body),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['sites', user, project] });
      navigate(`/projects/${user}/${project}/sites/${data.name}`);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    create.mutate({
      name: f.name.value,
      label: f.label.value,
      version: f.version.value,
      dataset: f.dataset.value,
    });
  };

  const projectPath = `/projects/${user}/${project}/settings`;

  return (
    <div className="form-page">
      <h2>New site</h2>
      <p className="form-help">A site is a deployment of a project version with an attached dataset.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" name="name" required pattern="[a-z][a-z0-9_-]*" placeholder="e.g. acme-prod" />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required placeholder="Display name" />
        </div>
        <div className="form-field">
          <label htmlFor="version">Version</label>
          <select id="version" name="version" required defaultValue={prefilledVersion}>
            <option value="" disabled>Select a version…</option>
            {versions.map(([id, fields]) => (
              <option key={id} value={fields.version}>
                {fields.version}
              </option>
            ))}
          </select>
        </div>
        <div className="form-field">
          <label htmlFor="dataset">Dataset</label>
          <select id="dataset" name="dataset" defaultValue="">
            <option value="">None</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
              </option>
            ))}
          </select>
        </div>
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(projectPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create site</button>
        </div>
      </form>
    </div>
  );
}
