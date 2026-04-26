//! Schema-aware validation for data records.
//!
//! loco-apps is the layer that gives meaning to collections and fields, so
//! validation lives here — not in loco-gen-schema (which is a generic codegen
//! lib) or loco-lake (which is intentionally schemaless).
//!
//! The output is an open-ended list of [`Diagnostic`]s rather than fixed
//! categories so future checks (regex/format, deno hooks, cross-field rules)
//! can extend the set without changing the shape.

use std::collections::HashMap;

use serde::Serialize;

use loco_lake::Value;

use crate::SchemaStore;

/// Stable string identifiers for the `kind` field on diagnostics. Clients can
/// switch on these. Using string constants (not an enum) keeps the set open
/// so plugins/hooks can add their own without enum churn.
pub mod kind {
    pub const UNKNOWN_FIELD: &str = "unknown_field";
    pub const TYPE_MISMATCH: &str = "type_mismatch";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(kind: &str, path: Option<String>, message: String) -> Self {
        Self {
            severity: Severity::Error,
            kind: kind.to_string(),
            path,
            message,
        }
    }

    pub fn warning(kind: &str, path: Option<String>, message: String) -> Self {
        Self {
            severity: Severity::Warning,
            kind: kind.to_string(),
            path,
            message,
        }
    }
}

/// How strict the validator should be. Same walk in all modes; only the
/// severity assigned to findings differs.
#[derive(Debug, Clone, Copy)]
pub enum ValidationMode {
    /// Full record going in for the first time. Findings are errors.
    Create,
    /// Partial patch — only fields present are checked. Findings are errors.
    Update,
    /// Reading existing data. Findings are warnings (drift is informational,
    /// never fails the request).
    Read,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ValidationReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Prefix every diagnostic's `path` with `record_id/`. Used by list
    /// handlers so per-record diagnostics carry their record id.
    pub fn prefix_paths(mut self, record_id: &str) -> Self {
        for d in &mut self.diagnostics {
            d.path = Some(match &d.path {
                Some(p) => format!("{record_id}/{p}"),
                None => record_id.to_string(),
            });
        }
        self
    }

    pub fn extend(&mut self, other: ValidationReport) {
        self.diagnostics.extend(other.diagnostics);
    }
}

/// Validate a record's fields against the schema for `(project, version, collection)`.
///
/// Looks up field definitions via `schema.fields().list(...)`; the record is
/// schemaless until this function correlates it with the schema in memory.
pub fn validate_record(
    schema: &SchemaStore,
    project_id: &str,
    version: &str,
    collection: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
) -> ValidationReport {
    let prefix = format!("{project_id}/versions/{version}/fields/{collection}/");
    let field_defs = schema.fields().list(&prefix);
    // Map of field name -> declared type string ("string", "integer", ...).
    let by_name: HashMap<&str, &str> = field_defs
        .iter()
        .map(|(_, f)| (f.name(), f.r#type()))
        .collect();

    let make = |kind: &str, path: Option<String>, message: String| match mode {
        ValidationMode::Create | ValidationMode::Update => Diagnostic::error(kind, path, message),
        ValidationMode::Read => Diagnostic::warning(kind, path, message),
    };

    let mut diagnostics = Vec::new();

    for (name, value) in fields {
        let Some(declared) = by_name.get(name.as_str()).copied() else {
            diagnostics.push(make(
                kind::UNKNOWN_FIELD,
                Some(name.clone()),
                format!(
                    "field '{name}' is not declared in collection '{collection}' (version {version})"
                ),
            ));
            continue;
        };

        if let Some(actual) = type_mismatch(declared, value) {
            diagnostics.push(make(
                kind::TYPE_MISMATCH,
                Some(name.clone()),
                format!("field '{name}' expected type '{declared}', got '{actual}'"),
            ));
        }
    }

    ValidationReport { diagnostics }
}

/// Return `None` if the value matches the declared type; otherwise the actual
/// type as a stable string. `Null` is currently allowed for any type — a
/// dedicated required-field check belongs alongside a future `required` flag
/// on `Field`.
///
/// Unknown declared types (anything outside string/integer/float/boolean) are
/// treated as "can't enforce" and pass — keeps us forward-compatible with new
/// type strings (e.g. "list") that the schema layer may add before the
/// validator catches up.
fn type_mismatch(declared: &str, value: &Value) -> Option<&'static str> {
    if matches!(value, Value::Null) {
        return None;
    }
    let actual = value_type_name(value);
    let ok = match declared {
        "string" => matches!(value, Value::String(_)),
        // JSON has no separate int vs float — accept either for a float field.
        "float" => matches!(value, Value::Float(_) | Value::Integer(_)),
        "integer" => matches!(value, Value::Integer(_)),
        "boolean" => matches!(value, Value::Boolean(_)),
        _ => return None,
    };
    if ok { None } else { Some(actual) }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::Null => "null",
    }
}

/// Convenience: validate every record in a list, prefixing each diagnostic's
/// path with the record id so call sites can flatten without losing context.
pub fn validate_records<'a, I>(
    schema: &SchemaStore,
    project_id: &str,
    version: &str,
    collection: &str,
    records: I,
    mode: ValidationMode,
) -> ValidationReport
where
    I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
{
    let mut combined = ValidationReport::default();
    for (id, fields) in records {
        let report = validate_record(schema, project_id, version, collection, fields, mode);
        if !report.is_empty() {
            combined.extend(report.prefix_paths(id));
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SchemaStore;
    use std::path::Path;

    fn load_schema() -> SchemaStore {
        // Use the repo's checked-in instances so we get a real account
        // collection with fields to validate against.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/instances");
        SchemaStore::load(&dir).expect("load schema fixtures")
    }

    /// The default fixtures define `ben/crm` at version `0.0.1-dev` with an
    /// `account` collection. This helper finds a real (project, version,
    /// collection) triple with at least one declared field, so tests are not
    /// brittle to specific fixture names.
    fn first_collection_with_fields(
        schema: &SchemaStore,
    ) -> (String, String, String, Vec<(String, String)>) {
        for (key, _) in schema.collections().list_all() {
            let vars = crate::Collection::from_path(&key).unwrap();
            let project = vars["project"].clone();
            let version = vars["version"].clone();
            let name = vars["name"].clone();
            let prefix = format!("{project}/versions/{version}/fields/{name}/");
            let fields: Vec<_> = schema
                .fields()
                .list(&prefix)
                .into_iter()
                .map(|(_, f)| (f.name().to_string(), f.r#type().to_string()))
                .collect();
            if !fields.is_empty() {
                return (project, version, name, fields);
            }
        }
        panic!("no collection with declared fields found in fixtures");
    }

    #[test]
    fn create_mode_emits_errors_for_unknown_fields() {
        let schema = load_schema();
        let (project, version, collection, _fields) = first_collection_with_fields(&schema);

        let mut data = HashMap::new();
        data.insert(
            "definitely_not_a_real_field_xyz".to_string(),
            Value::String("hi".into()),
        );

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Create,
        );
        assert!(report.has_errors());
        let d = report
            .diagnostics
            .iter()
            .find(|d| d.kind == kind::UNKNOWN_FIELD)
            .expect("expected an unknown_field diagnostic");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.path.as_deref(), Some("definitely_not_a_real_field_xyz"));
    }

    #[test]
    fn read_mode_downgrades_to_warning() {
        let schema = load_schema();
        let (project, version, collection, _fields) = first_collection_with_fields(&schema);

        let mut data = HashMap::new();
        data.insert("not_a_real_field_zzz".to_string(), Value::String("hi".into()));

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Read,
        );
        assert!(!report.has_errors());
        assert!(report
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning));
    }

    #[test]
    fn declared_field_with_matching_type_passes() {
        let schema = load_schema();
        let (project, version, collection, fields) = first_collection_with_fields(&schema);

        let mut data = HashMap::new();
        for (fname, ftype) in &fields {
            let v = match ftype.as_str() {
                "string" => Value::String("ok".into()),
                "integer" => Value::Integer(1),
                "float" => Value::Float(1.0),
                "boolean" => Value::Boolean(true),
                _ => continue,
            };
            data.insert(fname.clone(), v);
        }

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Create,
        );
        assert!(report.is_empty(), "unexpected diagnostics: {:?}", report);
    }

    #[test]
    fn type_mismatch_is_caught() {
        let schema = load_schema();
        let (project, version, collection, fields) = first_collection_with_fields(&schema);

        // Pick a known string field and send an integer.
        let Some((string_field, _)) = fields.iter().find(|(_, t)| t == "string") else {
            // Fixture doesn't have a string field; skip rather than fail.
            return;
        };

        let mut data = HashMap::new();
        data.insert(string_field.clone(), Value::Integer(42));

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Create,
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.kind == kind::TYPE_MISMATCH && d.path.as_deref() == Some(string_field)));
    }

    #[test]
    fn null_passes_for_any_type() {
        let schema = load_schema();
        let (project, version, collection, fields) = first_collection_with_fields(&schema);

        let mut data = HashMap::new();
        for (fname, _) in &fields {
            data.insert(fname.clone(), Value::Null);
        }

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Create,
        );
        assert!(report.is_empty());
    }

    #[test]
    fn integer_accepted_for_float_field() {
        // Direct unit test on the type matcher — JSON 1 should pass for float.
        assert!(type_mismatch("float", &Value::Integer(1)).is_none());
        assert!(type_mismatch("float", &Value::Float(1.0)).is_none());
        assert_eq!(type_mismatch("integer", &Value::Float(1.5)), Some("float"));
    }

    #[test]
    fn prefix_paths_attaches_record_id() {
        let mut report = ValidationReport {
            diagnostics: vec![
                Diagnostic::error("k", Some("a".into()), "m1".into()),
                Diagnostic::error("k", None, "m2".into()),
            ],
        };
        report = report.prefix_paths("rec-1");
        assert_eq!(report.diagnostics[0].path.as_deref(), Some("rec-1/a"));
        assert_eq!(report.diagnostics[1].path.as_deref(), Some("rec-1"));
    }

    #[test]
    fn empty_fields_produces_empty_report() {
        let schema = load_schema();
        let (project, version, collection, _) = first_collection_with_fields(&schema);
        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &HashMap::new(),
            ValidationMode::Create,
        );
        assert!(report.is_empty());
        assert!(!report.has_errors());
    }

    #[test]
    fn unknown_collection_marks_every_field_unknown() {
        // No such collection has any field defs — so every field in the
        // payload should come back as `unknown_field`.
        let schema = load_schema();
        let mut data = HashMap::new();
        data.insert("a".to_string(), Value::String("x".into()));
        data.insert("b".to_string(), Value::Integer(1));
        data.insert("c".to_string(), Value::Boolean(true));

        let report = validate_record(
            &schema,
            "ben/crm",
            "0.0.1-dev",
            "no_such_collection_zzz",
            &data,
            ValidationMode::Create,
        );
        assert_eq!(report.diagnostics.len(), 3);
        assert!(report
            .diagnostics
            .iter()
            .all(|d| d.kind == kind::UNKNOWN_FIELD && d.severity == Severity::Error));
    }

    #[test]
    fn update_mode_emits_errors_not_warnings() {
        let schema = load_schema();
        let (project, version, collection, _) = first_collection_with_fields(&schema);
        let mut data = HashMap::new();
        data.insert("not_real".to_string(), Value::String("x".into()));
        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Update,
        );
        assert!(report.has_errors());
        assert!(report
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn multiple_issues_in_single_record_all_reported() {
        let schema = load_schema();
        let (project, version, collection, fields) = first_collection_with_fields(&schema);
        let Some((string_field, _)) = fields.iter().find(|(_, t)| t == "string") else {
            return;
        };

        let mut data = HashMap::new();
        // type mismatch on a real field
        data.insert(string_field.clone(), Value::Integer(42));
        // two unknown fields
        data.insert("ghost1".to_string(), Value::String("a".into()));
        data.insert("ghost2".to_string(), Value::Boolean(true));

        let report = validate_record(
            &schema,
            &project,
            &version,
            &collection,
            &data,
            ValidationMode::Create,
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|d| d.kind == kind::UNKNOWN_FIELD)
                .count(),
            2
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|d| d.kind == kind::TYPE_MISMATCH)
                .count(),
            1
        );
    }

    #[test]
    fn validate_records_aggregates_and_prefixes_paths() {
        let schema = load_schema();
        let (project, version, collection, _) = first_collection_with_fields(&schema);

        let mut a = HashMap::new();
        a.insert("ghost_a".to_string(), Value::String("x".into()));
        let mut b = HashMap::new();
        b.insert("ghost_b".to_string(), Value::String("y".into()));
        let records = [("rec-A", &a), ("rec-B", &b)];

        let report = validate_records(
            &schema,
            &project,
            &version,
            &collection,
            records.iter().map(|(id, f)| (*id, *f)),
            ValidationMode::Read,
        );
        assert_eq!(report.diagnostics.len(), 2);
        let paths: Vec<_> = report
            .diagnostics
            .iter()
            .map(|d| d.path.as_deref().unwrap_or("").to_string())
            .collect();
        assert!(paths.contains(&"rec-A/ghost_a".to_string()));
        assert!(paths.contains(&"rec-B/ghost_b".to_string()));
        // Read mode → all warnings.
        assert!(report
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning));
    }

    #[test]
    fn validate_records_skips_clean_records() {
        let schema = load_schema();
        let (project, version, collection, fields) = first_collection_with_fields(&schema);
        let mut clean = HashMap::new();
        for (fname, ftype) in &fields {
            let v = match ftype.as_str() {
                "string" => Value::String("ok".into()),
                "integer" => Value::Integer(1),
                "float" => Value::Float(1.0),
                "boolean" => Value::Boolean(true),
                _ => continue,
            };
            clean.insert(fname.clone(), v);
        }
        let records = [("rec-clean", &clean)];
        let report = validate_records(
            &schema,
            &project,
            &version,
            &collection,
            records.iter().map(|(id, f)| (*id, *f)),
            ValidationMode::Read,
        );
        assert!(report.is_empty());
    }

    #[test]
    fn unknown_declared_type_is_not_enforced() {
        // A future type string we don't know about should NOT trigger a
        // type_mismatch — we'd rather pass-through than reject blindly.
        assert!(type_mismatch("list", &Value::String("x".into())).is_none());
        assert!(type_mismatch("date", &Value::Integer(1)).is_none());
    }

    #[test]
    fn type_name_strings_are_stable() {
        assert_eq!(value_type_name(&Value::String("x".into())), "string");
        assert_eq!(value_type_name(&Value::Integer(1)), "integer");
        assert_eq!(value_type_name(&Value::Float(1.0)), "float");
        assert_eq!(value_type_name(&Value::Boolean(true)), "boolean");
        assert_eq!(value_type_name(&Value::Null), "null");
    }

    #[test]
    fn severity_serializes_as_lowercase_string() {
        let json = serde_json::to_string(&Severity::Error).unwrap();
        assert_eq!(json, "\"error\"");
        let json = serde_json::to_string(&Severity::Warning).unwrap();
        assert_eq!(json, "\"warning\"");
        let json = serde_json::to_string(&Severity::Info).unwrap();
        assert_eq!(json, "\"info\"");
    }

    #[test]
    fn diagnostic_serializes_with_expected_keys() {
        let d = Diagnostic::warning("type_mismatch", Some("foo".into()), "bad".into());
        let v: serde_json::Value = serde_json::to_value(&d).unwrap();
        assert_eq!(v["severity"], "warning");
        assert_eq!(v["kind"], "type_mismatch");
        assert_eq!(v["path"], "foo");
        assert_eq!(v["message"], "bad");
    }

    #[test]
    fn diagnostic_omits_path_when_none() {
        let d = Diagnostic::error("hook_failed", None, "boom".into());
        let v: serde_json::Value = serde_json::to_value(&d).unwrap();
        assert!(v.get("path").is_none(), "path should be skipped when None");
    }

    #[test]
    fn kind_constants_are_stable_strings() {
        // These strings are part of the public API — clients switch on them.
        // If you rename one, you're making a breaking change.
        assert_eq!(kind::UNKNOWN_FIELD, "unknown_field");
        assert_eq!(kind::TYPE_MISMATCH, "type_mismatch");
    }

    #[test]
    fn float_field_rejects_non_number() {
        assert_eq!(
            type_mismatch("float", &Value::String("1.0".into())),
            Some("string")
        );
        assert_eq!(
            type_mismatch("float", &Value::Boolean(true)),
            Some("boolean")
        );
    }

    #[test]
    fn integer_field_rejects_float() {
        assert_eq!(type_mismatch("integer", &Value::Float(1.5)), Some("float"));
    }

    #[test]
    fn boolean_field_rejects_string_yes() {
        assert_eq!(
            type_mismatch("boolean", &Value::String("true".into())),
            Some("string")
        );
    }
}
