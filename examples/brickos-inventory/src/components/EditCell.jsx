import { useEffect, useState } from 'react';

/**
 * An input that saves one field on blur or Enter; Escape reverts.
 * `kind` is `text`, `int`, or `select` (with `options`). An `int` that does
 * not parse is not sent — the server would reject it anyway.
 */
export default function EditCell({ value, kind = 'text', options, list, onSave, ...rest }) {
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
        {!options.includes(draft) && <option value={draft}>{draft || '—'}</option>}
        {options.map((o) => (
          <option key={o}>{o}</option>
        ))}
      </select>
    );
  }

  return (
    <input
      className={invalid ? 'invalid' : undefined}
      value={draft}
      inputMode={kind === 'int' ? 'numeric' : undefined}
      list={list}
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
