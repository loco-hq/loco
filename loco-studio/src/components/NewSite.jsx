import { useState } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { TextField, SelectField } from 'loco-ui';
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

  const [name, setName] = useState('');
  const [label, setLabel] = useState('');
  const [version, setVersion] = useState(prefilledVersion);
  const [dataset, setDataset] = useState('');

  const create = useMutation({
    mutationFn: (body) => createSite(user, project, body),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['sites', user, project] });
      navigate(`/projects/${user}/${project}/sites/${data.name}`);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    create.mutate({ name, label, version, dataset });
  };

  const projectPath = `/projects/${user}/${project}/settings`;

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
      <h2>New site</h2>
      <p className="form-help">A site is a deployment of a project version with an attached dataset.</p>
      <form onSubmit={handleSubmit}>
        <TextField
          label="Name"
          required
          pattern="[a-z][a-z0-9_-]*"
          placeholder="e.g. acme-prod"
          value={name}
          onChange={setName}
        />
        <TextField
          label="Label"
          required
          placeholder="Display name"
          value={label}
          onChange={setLabel}
        />
        <SelectField
          label="Version"
          required
          placeholder="Select a version…"
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
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(projectPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create site</button>
        </div>
      </form>
    </div>
  );
}
