import { TextField } from './TextField.jsx';
import { NumberField } from './NumberField.jsx';
import { CheckboxField } from './CheckboxField.jsx';
import { ToggleField } from './ToggleField.jsx';
import { SelectField } from './SelectField.jsx';

const REGISTRY = {
  string:  { default: TextField },
  integer: { default: NumberField, _props: { integer: true } },
  float:   { default: NumberField },
  boolean: { default: CheckboxField, toggle: ToggleField },
};

export function Field({ field, variant, value, onChange, ...rest }) {
  const entry = REGISTRY[field.type];
  if (!entry) {
    return (
      <div style={{ color: 'var(--loco-color-text-danger)', fontFamily: 'var(--loco-font)' }}>
        Unknown field type: <code>{field.type}</code>
      </div>
    );
  }

  // A string field with options is a choice, whatever its variant. The
  // blank entry stands for null — the server rejects '' as an option.
  const hasOptions = field.type === 'string' && field.options?.length > 0;
  const chosenVariant = variant ?? field.variant;
  const Component = hasOptions ? SelectField : entry[chosenVariant] ?? entry.default;
  const typeProps = hasOptions
    ? { options: field.options, placeholder: '—' }
    : entry._props ?? {};

  // A required field must not be left blank. A checkbox never is: unchecked
  // is false, a value. The native `required` on one means "must be checked",
  // so a boolean does not pass it on.
  const required = field.type !== 'boolean' && field.required;

  return (
    <Component
      label={field.label ?? field.name}
      description={field.description}
      required={required}
      value={value}
      onChange={onChange}
      {...typeProps}
      {...rest}
    />
  );
}
