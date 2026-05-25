import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { TextField, SelectField } from 'loco-ui';
import { getSite, updateSite, listDatasets, listVersions } from '../api.js';

export default function EditSite() {
  const { user, project, name } = useParams();

  const { data: site, isLoading, error } = useQuery({
    queryKey: ['site', user, project, name],
    queryFn: () => getSite(user, project, name),
  });

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return <EditSiteForm site={site} />;
}

function EditSiteForm({ site }) {
  const { user, project, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const sitePath = `/projects/${user}/${project}/sites/${name}`;

  const { data: datasets = [] } = useQuery({
    queryKey: ['datasets', user, project],
    queryFn: () => listDatasets(user, project),
  });

  const { data: versions = [] } = useQuery({
    queryKey: ['versions', user, project],
    queryFn: () => listVersions(user, project),
  });

  const [label, setLabel] = useState(site.label || '');
  const [version, setVersion] = useState(site.version || '');
  const [dataset, setDataset] = useState(site.dataset || '');

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
    update.mutate({ label, version, dataset });
  };

  const versionOptions = versions.map(([, fields]) => ({
    value: fields.version,
    label: fields.version,
  }));
  const datasetOptions = datasets.map(([, fields]) => ({
    value: fields.name || '',
    label: fields.label || fields.name,
  }));

  return (
    <div className="form-page">
      <h2>Edit site</h2>
      <p className="form-help">Site name is immutable. Update the label, version, or dataset binding.</p>
      <form onSubmit={handleSubmit}>
        <TextField label="Name" value={site.name} onChange={() => {}} disabled />
        <TextField
          label="Label"
          required
          value={label}
          onChange={setLabel}
        />
        <SelectField
          label="Version"
          required
          options={versionOptions}
          value={version}
          onChange={setVersion}
        />
        <SelectField
          label="Dataset"
          placeholder="None"
          options={datasetOptions}
          value={dataset}
          onChange={setDataset}
        />
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(sitePath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
