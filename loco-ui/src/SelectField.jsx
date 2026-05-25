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
        value={value ?? ''}
        onChange={(e) => onChange?.(e.target.value)}
        disabled={disabled}
        required={required}
        {...shellAriaProps({ id, description, error })}
      >
        {placeholder !== undefined ? (
          <option value="" disabled={required}>{placeholder}</option>
        ) : null}
        {options.map((opt) => {
          const o = typeof opt === 'string' ? { value: opt, label: opt } : opt;
          return (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label ?? o.value}
            </option>
          );
        })}
      </select>
    </FieldShell>
  );
}
