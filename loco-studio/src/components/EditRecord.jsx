import { useState, useEffect } from 'react';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getRecord, updateRecord, listFields } from '../api.js';
import { FieldInput, buildPayload } from './recordFields.jsx';
import { encodeId, decodeId } from '../shortId.js';

export default function EditRecord() {
  const { user, project, version, name: collection, recordId: shortId } = useParams();
  const recordId = decodeId(shortId);
  const [searchParams] = useSearchParams();
  const siteName = searchParams.get('site');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const projectId = `${user}/${project}`;
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${collection}`;
  const siteParam = siteName ? `?site=${encodeURIComponent(siteName)}` : '';
  const recordPath = `${collectionPath}/records/${shortId}${siteParam}`;

  const { data: record, isLoading, error } = useQuery({
    queryKey: ['record', projectId, siteName, collection, recordId],
    queryFn: () => getRecord(projectId, siteName, collection, recordId),
    enabled: !!siteName,
  });

  const { data: fields = [] } = useQuery({
    queryKey: ['fields', user, project, version, collection],
    queryFn: () => listFields(user, project, version, collection),
  });

  const [draft, setDraft] = useState(null);

  useEffect(() => {
    if (record && draft === null) {
      setDraft({ ...record.fields });
    }
  }, [record, draft]);

  const update = useMutation({
    mutationFn: () =>
      updateRecord(projectId, siteName, collection, recordId, buildPayload(fields, draft)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['record', projectId, siteName, collection, recordId] });
      qc.invalidateQueries({ queryKey: ['records', projectId, siteName, collection] });
      navigate(recordPath);
    },
  });

  if (!siteName) {
    return <p className="error">Missing site context.</p>;
  }
  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading || !record || draft === null) return <p>Loading...</p>;

  return (
    <div className="form-page">
      <h2>Edit record</h2>
      <p className="form-help">Site: <code>{siteName}</code> · ID: <code>{encodeId(record.id)}</code></p>
      <form onSubmit={(e) => { e.preventDefault(); update.mutate(); }}>
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
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(recordPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
