import { FieldShell, shellAriaProps } from './_shell/FieldShell.jsx';
import { useFieldId } from './_shell/useFieldId.js';
import controlStyles from './_shell/control.module.css';

export function TextField({
  id: providedId,
  label,
  description,
  error,
  required,
  disabled,
  value,
  onChange,
  placeholder,
  type = 'text',
  ...rest
}) {
  const id = useFieldId(providedId);
  const className = error
    ? `${controlStyles.input} ${controlStyles.invalid}`
    : controlStyles.input;

  return (
    <FieldShell
      id={id}
      label={label}
      description={description}
      error={error}
      required={required}
    >
      <input
        {...rest}
        id={id}
        type={type}
        className={className}
        value={value ?? ''}
        onChange={(e) => onChange?.(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        required={required}
        {...shellAriaProps({ id, description, error })}
      />
    </FieldShell>
  );
}
