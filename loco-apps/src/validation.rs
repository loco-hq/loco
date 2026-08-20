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

use crate::http::version_schema::VersionSchema;

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

/// Validate a record's fields against the schema for a collection.
///
/// `schema.fields(collection)` returns the union of fields targeting this
/// collection across every installed dependency — extensions declared by
/// other deps are picked up automatically.
pub fn validate_record(
    schema: &VersionSchema,
    collection: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
) -> ValidationReport {
    let field_defs = schema.fields(collection);
    // Map of field name -> declared type string ("string", "integer", ...).
    let by_name: HashMap<&str, &str> = field_defs.iter().map(|f| (f.name(), f.r#type())).collect();

    let make = |kind: &str, path: Option<String>, message: String| match mode {
        ValidationMode::Create | ValidationMode::Update => Diagnostic::error(kind, path, message),
        ValidationMode::Read => Diagnostic::warning(kind, path, message),
    };

    let version = schema.version();
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
    if ok {
        None
    } else {
        Some(actual)
    }
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
    schema: &VersionSchema,
    collection: &str,
    records: I,
    mode: ValidationMode,
) -> ValidationReport
where
    I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
{
    let mut combined = ValidationReport::default();
    for (id, fields) in records {
        let report = validate_record(schema, collection, fields, mode);
        if !report.is_empty() {
            combined.extend(report.prefix_paths(id));
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

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
