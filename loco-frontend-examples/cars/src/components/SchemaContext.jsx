import { createContext, useContext, useState, useEffect } from 'react';
import { getSchemaCollections } from '../loco.js';

const SchemaContext = createContext(null);

export function SchemaProvider({ children }) {
  const [collections, setCollections] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    getSchemaCollections()
      .then((schema) => {
        const cols = schema.flatMap((ns) => {
          const [user, project] = ns.namespace.split('/');
          return ns.collections.map((col) => ({
            user,
            project,
            namespace: ns.namespace,
            name: col.name,
            label: col.fields.label || col.name,
            fields: col.collection_fields.map(([, f]) => ({
              name: f.name,
              type: f.type,
            })),
          }));
        });
        setCollections(cols);
      })
      .catch((err) => setError(err.message));
  }, []);

  return (
    <SchemaContext.Provider value={{ collections, error }}>
      {children}
    </SchemaContext.Provider>
  );
}

export function useSchema() {
  return useContext(SchemaContext);
}
