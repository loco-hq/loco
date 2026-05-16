import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getCollection, updateCollection } from '../api.js';

export default function EditCollection() {
  const { user, project, version, name } = useParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const collectionPath = `/projects/${user}/${project}/versions/${version}/collections/${name}`;

  const { data: collection, isLoading, error } = useQuery({
    queryKey: ['collection', user, project, version, name],
    queryFn: () => getCollection(user, project, version, name),
  });

  const update = useMutation({
    mutationFn: (patch) => updateCollection(user, project, version, name, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['collection', user, project, version, name] });
      qc.invalidateQueries({ queryKey: ['collections', user, project, version] });
      navigate(collectionPath);
    },
  });

  const handleSubmit = (e) => {
    e.preventDefault();
    const f = e.target.elements;
    update.mutate({
      label: f.label.value,
      label_plural: f.label_plural.value,
    });
  };

  if (error) return <p className="error">Error: {error.message}</p>;
  if (isLoading) return <p>Loading...</p>;

  return (
    <div className="form-page">
      <h2>Edit collection</h2>
      <p className="form-help">Collection name is immutable. Update the labels.</p>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="name">Name</label>
          <input id="name" value={collection.name} disabled />
        </div>
        <div className="form-field">
          <label htmlFor="label">Label</label>
          <input id="label" name="label" required defaultValue={collection.label || ''} />
        </div>
        <div className="form-field">
          <label htmlFor="label_plural">Plural label</label>
          <input id="label_plural" name="label_plural" required defaultValue={collection.label_plural || ''} />
        </div>
        {update.error && <p className="error">{update.error.message}</p>}
        <div className="form-actions">
          <button type="button" onClick={() => navigate(collectionPath)}>Cancel</button>
          <button type="submit" disabled={update.isPending}>Save changes</button>
        </div>
      </form>
    </div>
  );
}
