import { TextField } from './TextField.jsx';
import { NumberField } from './NumberField.jsx';
import { CheckboxField } from './CheckboxField.jsx';
import { ToggleField } from './ToggleField.jsx';

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

  const chosenVariant = variant ?? field.variant;
  const Component = entry[chosenVariant] ?? entry.default;
  const typeProps = entry._props ?? {};

  return (
    <Component
      label={field.label ?? field.name}
      description={field.description}
      required={field.required}
      value={value}
      onChange={onChange}
      {...typeProps}
      {...rest}
    />
  );
}
