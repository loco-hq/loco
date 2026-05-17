import { useState, useEffect } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listFields, addRecord, getCollection } from '../api.js';
import { FieldInput, initialDraft, buildPayload } from './recordFields.jsx';

export default function NewRecord() {
  const { user, project, version, name: collection } = useParams();
  const [searchParams] = useSearchParams();
  const siteName = searchParams.get('site');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const projectId = `${user}/${project}`;
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;

  const { data: coll } = useQuery({
    queryKey: ['collection', user, project, version, collection],
    queryFn: () => getCollection(user, project, version, collection),
  });

  const { data: fields = [], isLoading, error } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  const [draft, setDraft] = useState(null);

  useEffect(() => {
    if (draft === null && fields.length > 0) {
      setDraft(initialDraft(fields));
    }
  }, [fields, draft]);

  const create = useMutation({
    mutationFn: () => addRecord(projectId, siteName, collection, buildPayload(fields, draft)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['records', projectId, siteName, collection] });
      navigate(collectionPath);
    },
  });

  if (!siteName) {
    return <p className="error">Missing site context. Open this page from a collection's Records section.</p>;
  }
  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading || draft === null) return <p>Loading...</p>;

  const label = coll?.label || collection;

  return (
    <div className="form-page">
      <h2>New {label}</h2>
      <p className="form-help">Site: <code>{siteName}</code></p>
      <form onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
        {fields.map((f) => (
          <div key={f.name} className="form-field">
            <label htmlFor={f.name}>{f.label || f.name}</label>
            <FieldInput
              field={f}
              value={draft[f.name]}
              onChange={(v) => setDraft({ ...draft, [f.name]: v })}
            />
          </div>
        ))}
        {create.error && <p className="error">{create.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(collectionPath)}>Cancel</button>
          <button type="submit" disabled={create.isPending}>Create {label}</button>
        </div>
      </form>
    </div>
  );
}
