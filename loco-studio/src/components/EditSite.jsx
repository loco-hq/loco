import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getSite, updateSite, listDatasets, listVersions } from '../api.js';

export default function EditSite() {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const sitePath = `/projects/${user}/${project}/sites/${name}`;

  const { data: site, isLoading, error } = useQuery({
    queryKey: ['site', user, project, name],
    queryFn: () => getSite(user, project, name),
  });

  const { data: datasets = [] } = useQuery({
    queryKey: ['datasets', user, project],
    queryFn: () => listDatasets(user, project),
  });

  const { data: versions = [] } = useQuery({
    queryKey: ['versions', user, project],
    queryFn: () => listVersions(user, project),
  });

  const update = useMutation({
    mutationFn: (patch) => updateSite(user, project, name, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['site', user, project, name] });
      qc.invalidateQueries({ queryKey: ['sites', user, project] });
      navigate(sitePath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    update.mutate({
      label: f.label.value,
      version: f.version.value,
      dataset: f.dataset.value,
    });
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <div className="form-page">
      <h2>Edit site</h2>
      <p className="form-help">Site name is immutable. Update the label, version, or dataset binding.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" value={site.name} disabled />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required defaultValue={site.label || ''} />
        </div>
        <div className="form-field">
          <label htmlFor="version">Version</label>
          <select id="version" name="version" required defaultValue={site.version || ''}>
            {versions.length === 0 && <option value="">No versions available</option>}
            {versions.map(([id, fields]) => (
              <option key={id} value={fields.version}>
                {fields.version}
              </option>
            ))}
          </select>
        </div>
        <div className="form-field">
          <label htmlFor="dataset">Dataset</label>
          <select id="dataset" name="dataset" defaultValue={site.dataset || ''}>
            <option value="">None</option>
            {datasets.map(([id, fields]) => (
              <option key={id} value={fields.name || ''}>
                {fields.label || fields.name}
              </option>
            ))}
          </select>
        </div>
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(sitePath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
