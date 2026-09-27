import { useEffect, useState } from 'react';

/**
 * An input that saves one field on blur or Enter; Escape reverts.
 * `kind` is `text`, `int`, or `select` (with a field's `options`). An `int` that does
 * not parse is not sent — the server would reject it anyway.
 */
export default function EditCell({ value, kind = 'text', options, onSave, ...rest }) {
  const shown = value ?? '';
  const [draft, setDraft] = useState(String(shown));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => setDraft(String(shown)), [shown]);

  function commit(next = draft) {
    if (next === String(shown)) return;
    if (kind === 'int') {
      if (!/^-?\d+$/.test(next.trim())) {
        setInvalid(true);
        return;
      }
      setInvalid(false);
      onSave(parseInt(next, 10));
    } else {
      onSave(next);
    }
  }

  if (kind === 'select') {
    return (
      <select
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          commit(e.target.value);
        }}
        {...rest}
      >
        {!options.some((o) => o.value === draft) && <option value={draft}>{draft || '—'}</option>}
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    );
  }

  return (
    <input
      className={invalid ? 'invalid' : undefined}
      value={draft}
      inputMode={kind === 'int' ? 'numeric' : undefined}
      onChange={(e) => {
        setDraft(e.target.value);
        setInvalid(false);
      }}
      onBlur={() => commit()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        if (e.key === 'Escape') {
          setDraft(String(shown));
          setInvalid(false);
        }
      }}
      {...rest}
    />
  );
}
