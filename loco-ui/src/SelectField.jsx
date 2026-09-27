import { FieldShell, shellAriaProps } from './_shell/FieldShell.jsx';
import { useFieldId } from './_shell/useFieldId.js';
import styles from './SelectField.module.css';

export function SelectField({
  id: providedId,
  label,
  description,
  error,
  required,
  disabled,
  value,
  onChange,
  options = [],
  placeholder,
  ...rest
}) {
  const id = useFieldId(providedId);
  const normalized = options.map((opt) => (typeof opt === 'string' ? { value: opt, label: opt } : opt));
  // A stored value that is not an option (it was removed, or the value
  // predates it) is still shown, marked, so the select does not silently
  // display another choice.
  const current = value === null || value === undefined ? '' : String(value);
  const drifted = current !== '' && !normalized.some((o) => String(o.value) === current);
  const isPlaceholder = (value ?? '') === '' && placeholder !== undefined;
  const classes = [styles.select];
  if (error) classes.push(styles.invalid);
  if (isPlaceholder) classes.push(styles.placeholder);

  return (
    <FieldShell
      id={id}
      label={label}
      description={description}
      error={error}
      required={required}
    >
      <select
        {...rest}
        id={id}
        className={classes.join(' ')}
        value={current}
        onChange={(e) => onChange?.(e.target.value)}
        disabled={disabled}
        required={required}
        {...shellAriaProps({ id, description, error })}
      >
        {placeholder !== undefined ? (
          <option value="" disabled={required}>{placeholder}</option>
        ) : null}
        {drifted ? <option value={current}>{current} (not an option)</option> : null}
        {normalized.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label ?? o.value}
          </option>
        ))}
      </select>
    </FieldShell>
  );
}
