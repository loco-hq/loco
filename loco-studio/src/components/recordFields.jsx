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
  return raw ?? '';
}

export function buildPayload(fields, draft) {
  const out = {};
  for (const f of fields) {
    out[f.name] = coerce(f, draft[f.name]);
  }
  return out;
}

export function displayValue(field, value) {
  if (value === null || value === undefined) return '—';
  if (field.type === 'boolean') return value ? '✓' : '✗';
  if (value === '') return '—';
  return String(value);
}
