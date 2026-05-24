import { useState } from 'react';
import {
  Field,
  TextField,
  NumberField,
  CheckboxField,
  ToggleField,
} from 'loco-ui';

function Section({ title, children }) {
  return (
    <section className="section">
      <h2>{title}</h2>
      <div className="grid">{children}</div>
    </section>
  );
}

function Cell({ label, children }) {
  return (
    <div className="cell">
      <div className="cell-label">{label}</div>
      {children}
    </div>
  );
}

export function App() {
  const [draft, setDraft] = useState({
    name: '',
    age: null,
    weight: null,
    active: false,
    notifications: true,
  });

  const set = (key) => (v) => setDraft((d) => ({ ...d, [key]: v }));

  const fields = [
    { name: 'name', type: 'string', label: 'Name', description: 'Your full name' },
    { name: 'age', type: 'integer', label: 'Age' },
    { name: 'weight', type: 'float', label: 'Weight (kg)' },
    { name: 'active', type: 'boolean', label: 'Active' },
    { name: 'notifications', type: 'boolean', variant: 'toggle', label: 'Notifications' },
  ];

  return (
    <div className="page">
      <h1>loco-ui playground</h1>

      <Section title="TextField">
        <Cell label="Default">
          <TextField label="Name" value="" onChange={() => {}} placeholder="Jane Doe" />
        </Cell>
        <Cell label="With description">
          <TextField
            label="Email"
            description="We'll never share it."
            value="jane@example.com"
            onChange={() => {}}
          />
        </Cell>
        <Cell label="Required + error">
          <TextField
            label="Slug"
            required
            error="Must be lowercase letters only"
            value="Bad Value"
            onChange={() => {}}
          />
        </Cell>
        <Cell label="Disabled">
          <TextField label="Locked" value="cannot edit" onChange={() => {}} disabled />
        </Cell>
      </Section>

      <Section title="NumberField">
        <Cell label="Float (default)">
          <NumberField label="Weight" value={72.5} onChange={() => {}} />
        </Cell>
        <Cell label="Integer">
          <NumberField label="Age" integer value={30} onChange={() => {}} min={0} max={150} />
        </Cell>
        <Cell label="Error">
          <NumberField label="Count" integer value={-3} onChange={() => {}} error="Must be positive" />
        </Cell>
        <Cell label="Disabled">
          <NumberField label="Locked" value={42} onChange={() => {}} disabled />
        </Cell>
      </Section>

      <Section title="CheckboxField">
        <Cell label="Unchecked">
          <CheckboxField label="Accept terms" value={false} onChange={() => {}} />
        </Cell>
        <Cell label="Checked">
          <CheckboxField label="Subscribe" value={true} onChange={() => {}} />
        </Cell>
        <Cell label="With description + error">
          <CheckboxField
            label="Required box"
            description="You must accept to continue"
            error="This box must be checked"
            value={false}
            onChange={() => {}}
          />
        </Cell>
        <Cell label="Disabled">
          <CheckboxField label="Locked" value={true} onChange={() => {}} disabled />
        </Cell>
      </Section>

      <Section title="ToggleField">
        <Cell label="Off">
          <ToggleField label="Dark mode" value={false} onChange={() => {}} />
        </Cell>
        <Cell label="On">
          <ToggleField label="Notifications" value={true} onChange={() => {}} />
        </Cell>
        <Cell label="With description">
          <ToggleField
            label="Auto-save"
            description="Save every 30 seconds"
            value={true}
            onChange={() => {}}
          />
        </Cell>
        <Cell label="Disabled">
          <ToggleField label="Locked" value={true} onChange={() => {}} disabled />
        </Cell>
      </Section>

      <Section title="Field dispatcher (live)">
        {fields.map((f) => (
          <Cell key={f.name} label={`type: ${f.type}${f.variant ? ` / ${f.variant}` : ''}`}>
            <Field field={f} value={draft[f.name]} onChange={set(f.name)} />
          </Cell>
        ))}
        <div style={{ gridColumn: '1 / -1' }}>
          <div className="payload">{JSON.stringify(draft, null, 2)}</div>
        </div>
      </Section>
    </div>
  );
}
