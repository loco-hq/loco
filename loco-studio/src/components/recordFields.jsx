export function initialDraft(fields) {
  const d = {};
  for (const f of fields) {
    d[f.name] = f.type === 'boolean' ? false : '';
  }
  return d;
}

export function coerce(field, raw) {
  if (field.type === 'boolean') return !!raw;
  if (field.type === 'integer') {
    if (raw === '' || raw === null || raw === undefined) return null;
    const n = parseInt(raw, 10);
    return Number.isNaN(n) ? null : n;
  }
  if (field.type === 'float') {
    if (raw === '' || raw === null || raw === undefined) return null;
    const n = parseFloat(raw);
    return Number.isNaN(n) ? null : n;
  }
  // No choice is null: '' is not one of a field's options.
  if (field.options?.length) return raw === '' || raw === undefined ? null : raw;
  return raw ?? '';
}

export function buildPayload(fields, draft) {
  const out = {};
  for (const f of fields) {
    out[f.name] = coerce(f, draft[f.name]);
  }
  return out;
}

// An update sends only the fields the user changed, so a stored value the
// schema no longer accepts (a removed option, a type drift) does not fail a
// save that never touched it.
export function buildChanges(fields, original, draft) {
  const out = {};
  for (const f of fields) {
    if (draft[f.name] !== original[f.name]) out[f.name] = coerce(f, draft[f.name]);
  }
  return out;
}

export function displayValue(field, value) {
  if (value === null || value === undefined) return '—';
  if (field.type === 'boolean') return value ? '✓' : '✗';
  if (value === '') return '—';
  return String(value);
}
