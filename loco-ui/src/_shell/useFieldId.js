import { useId } from 'react';

export function useFieldId(provided) {
  const auto = useId();
  return provided ?? `loco-field-${auto.replace(/:/g, '')}`;
}
