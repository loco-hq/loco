import { FieldShell, shellAriaProps } from './_shell/FieldShell.jsx';
import { useFieldId } from './_shell/useFieldId.js';
import controlStyles from './_shell/control.module.css';

export function NumberField({
  id: providedId,
  label,
  description,
  error,
  required,
  disabled,
  value,
  onChange,
  placeholder,
  integer = false,
  min,
  max,
  step,
  ...rest
}) {
  const id = useFieldId(providedId);
  const className = error
    ? `${controlStyles.input} ${controlStyles.invalid}`
    : controlStyles.input;

  const resolvedStep = step ?? (integer ? 1 : 'any');

  const handleChange = (e) => {
    const raw = e.target.value;
    if (raw === '') return onChange?.(null);
    const n = integer ? parseInt(raw, 10) : parseFloat(raw);
    onChange?.(Number.isNaN(n) ? null : n);
  };

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
        type="number"
        className={className}
        value={value ?? ''}
        onChange={handleChange}
        placeholder={placeholder}
        disabled={disabled}
        required={required}
        min={min}
        max={max}
        step={resolvedStep}
        {...shellAriaProps({ id, description, error })}
      />
    </FieldShell>
  );
}
