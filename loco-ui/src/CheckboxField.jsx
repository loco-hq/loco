import { FieldShell, shellAriaProps } from './_shell/FieldShell.jsx';
import { useFieldId } from './_shell/useFieldId.js';
import styles from './CheckboxField.module.css';

export function CheckboxField({
  id: providedId,
  label,
  description,
  error,
  required,
  disabled,
  value,
  onChange,
  ...rest
}) {
  const id = useFieldId(providedId);
  const className = error
    ? `${styles.checkbox} ${styles.invalid}`
    : styles.checkbox;

  return (
    <FieldShell
      id={id}
      label={label}
      description={description}
      error={error}
      required={required}
      layout="inline"
    >
      <input
        {...rest}
        id={id}
        type="checkbox"
        className={className}
        checked={!!value}
        onChange={(e) => onChange?.(e.target.checked)}
        disabled={disabled}
        required={required}
        {...shellAriaProps({ id, description, error })}
      />
    </FieldShell>
  );
}
