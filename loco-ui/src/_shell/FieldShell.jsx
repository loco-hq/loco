import styles from './FieldShell.module.css';

export function FieldShell({
  id,
  label,
  description,
  error,
  required,
  layout = 'stacked',
  children,
}) {
  const isInline = layout === 'inline';
  const descId = description ? `${id}-desc` : undefined;
  const errId = error ? `${id}-err` : undefined;

  const labelEl = label ? (
    <label htmlFor={id} className={styles.label}>
      {label}
      {required ? <span className={styles.required} aria-hidden="true">*</span> : null}
    </label>
  ) : null;

  return (
    <div className={styles.shell}>
      {isInline ? (
        <div className={styles.row}>
          {children}
          {labelEl}
        </div>
      ) : (
        <>
          {labelEl}
          {children}
        </>
      )}
      {description ? (
        <div id={descId} className={styles.description}>{description}</div>
      ) : null}
      {error ? (
        <div id={errId} className={styles.error} role="alert">{error}</div>
      ) : null}
    </div>
  );
}

export function shellAriaProps({ id, description, error }) {
  const describedBy = [
    description ? `${id}-desc` : null,
    error ? `${id}-err` : null,
  ].filter(Boolean).join(' ') || undefined;
  return {
    'aria-describedby': describedBy,
    'aria-invalid': error ? true : undefined,
  };
}
