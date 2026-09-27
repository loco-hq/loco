// The types a collection field may declare. The server rejects anything else
// (FIELD_TYPES in loco-apps/src/validation.rs); keep the two in step.
export const FIELD_TYPES = ['string', 'integer', 'float', 'boolean'];

export const TYPE_OPTIONS = FIELD_TYPES.map((t) => ({ value: t, label: t }));
