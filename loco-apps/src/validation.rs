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

use crate::http::version_schema::{InlineField, VersionSchema};
use crate::{Action, IntegrationAction, IntegrationActionParam};

/// Stable string identifiers for the `kind` field on diagnostics. Clients can
/// switch on these. Using string constants (not an enum) keeps the set open
/// so plugins/hooks can add their own without enum churn.
pub mod kind {
    pub const UNKNOWN_FIELD: &str = "unknown_field";
    pub const TYPE_MISMATCH: &str = "type_mismatch";
    pub const INVALID_OPTION: &str = "invalid_option";
    pub const REQUIRED: &str = "required";
    /// A verb, filter, order, limit, or cursor the collection's source does
    /// not declare. The result is this error, never a widened page.
    pub const UNSUPPORTED: &str = "unsupported";
    /// The integration source's upstream call failed. The message is
    /// `upstream {status}: {message}` and does not include the request URL.
    pub const UPSTREAM: &str = "upstream";
    /// Reading connection values needs `LOCO_SECRET_KEY`, and it is unset
    /// or malformed.
    pub const UNAVAILABLE: &str = "unavailable";
    /// The source failed, or a lake read of this integration's connection
    /// values failed. On a query the latter stays inside that query.
    pub const FAILED: &str = "failed";
}

/// The types a collection field may declare. `/schema` field writes reject
/// anything else; these are exactly the types [`type_mismatch`] enforces and
/// the lake `Value` can hold.
pub const FIELD_TYPES: [&str; 4] = ["string", "integer", "float", "boolean"];

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
    /// Full record going in for the first time. Findings are errors, and a
    /// required field that is absent is one.
    Create,
    /// Partial patch — only fields present are checked, so a patch need not
    /// resend every required field. Findings are errors.
    Update,
    /// Reading existing data, a whole record like `Create`. Findings are
    /// warnings (drift is informational, never fails the request) — a field
    /// made required after records were written is reported, not enforced.
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

/// Validate a record's fields against the collection `collection` owned by
/// `owner`: the fields its owner declares, and only those — no other
/// project adds to a collection it does not own ([`VersionSchema::fields_of`]).
pub fn validate_record(
    schema: &VersionSchema,
    owner: &str,
    collection: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
) -> ValidationReport {
    check_record(schema, owner, collection, fields, mode, None)
}

/// [`validate_record`], where `projection` names the only fields the record
/// was read with (a `/data/query` `fields` list): a required field outside it
/// is absent because it was not asked for, not because it is unset.
fn check_record(
    schema: &VersionSchema,
    owner: &str,
    collection: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
    projection: Option<&[String]>,
) -> ValidationReport {
    let field_defs = schema.fields_of(owner, collection);
    let collection = schema.reference(owner, collection);
    let specs: Vec<ScalarSpec> = field_defs
        .iter()
        .map(|field| ScalarSpec {
            name: field.name(),
            ty: field.r#type(),
            required: field.required,
            options: field.options().iter().map(|o| o.value()).collect(),
        })
        .collect();
    check_scalars(
        &specs,
        fields,
        &[],
        mode,
        Wording {
            noun: "field",
            container: &format!("collection '{collection}'"),
            version: schema.version(),
        },
        projection,
    )
}

/// One declared scalar, whether it is a collection field or an action param.
struct ScalarSpec<'a> {
    name: &'a str,
    ty: &'a str,
    required: bool,
    options: Vec<&'a str>,
}

/// The words that distinguish a field diagnostic from a param diagnostic.
/// The `kind` strings stay the `/data` ones either way.
#[derive(Clone, Copy)]
struct Wording<'a> {
    noun: &'a str,
    container: &'a str,
    version: &'a str,
}

/// `check_record` and [`validate_action_input`] share this walk. `non_scalars`
/// are input values the lake `Value` cannot hold (a JSON array or object);
/// record checks pass an empty list because `/data` already rejected those
/// at the body parser.
fn check_scalars(
    specs: &[ScalarSpec<'_>],
    fields: &HashMap<String, Value>,
    non_scalars: &[(String, &'static str)],
    mode: ValidationMode,
    wording: Wording<'_>,
    projection: Option<&[String]>,
) -> ValidationReport {
    let by_name: HashMap<&str, &ScalarSpec> = specs.iter().map(|spec| (spec.name, spec)).collect();

    let make = |kind: &str, path: Option<String>, message: String| match mode {
        ValidationMode::Create | ValidationMode::Update => Diagnostic::error(kind, path, message),
        ValidationMode::Read => Diagnostic::warning(kind, path, message),
    };
    let unknown = |name: &str| {
        make(
            kind::UNKNOWN_FIELD,
            Some(name.to_string()),
            format!(
                "{} '{name}' is not declared in {} (version {})",
                wording.noun, wording.container, wording.version
            ),
        )
    };

    let mut diagnostics = Vec::new();

    for (name, value) in fields {
        let Some(spec) = by_name.get(name.as_str()).copied() else {
            diagnostics.push(unknown(name));
            continue;
        };
        let declared = spec.ty;
        if spec.required && is_blank(declared, value) {
            diagnostics.push(make(
                kind::REQUIRED,
                Some(name.clone()),
                format!("{} '{name}' is required", wording.noun),
            ));
        } else if let Some(actual) = type_mismatch(declared, value) {
            diagnostics.push(make(
                kind::TYPE_MISMATCH,
                Some(name.clone()),
                format!(
                    "{} '{name}' expected type '{declared}', got '{actual}'",
                    wording.noun
                ),
            ));
        } else if let Some(given) = invalid_option(declared, &spec.options, value) {
            diagnostics.push(make(
                kind::INVALID_OPTION,
                Some(name.clone()),
                format!(
                    "{} '{name}' must be one of [{}], got '{given}'",
                    wording.noun,
                    spec.options.join(", ")
                ),
            ));
        }
    }

    for (name, actual) in non_scalars {
        let Some(spec) = by_name.get(name.as_str()).copied() else {
            diagnostics.push(unknown(name));
            continue;
        };
        diagnostics.push(make(
            kind::TYPE_MISMATCH,
            Some(name.clone()),
            format!(
                "{} '{name}' expected type '{}', got '{actual}'",
                wording.noun, spec.ty
            ),
        ));
    }

    if !matches!(mode, ValidationMode::Update) {
        for spec in specs.iter().filter(|spec| spec.required) {
            let present = fields.contains_key(spec.name)
                || non_scalars.iter().any(|(name, _)| name == spec.name);
            let read = projection.is_none_or(|p| p.iter().any(|n| n == spec.name));
            if read && !present {
                diagnostics.push(make(
                    kind::REQUIRED,
                    Some(spec.name.to_string()),
                    format!("{} '{}' is required", wording.noun, spec.name),
                ));
            }
        }
    }

    ValidationReport { diagnostics }
}

/// Whether `value` leaves a required scalar unfilled: `Null` for any type, and
/// `""` for a string. An empty string is what a cleared text input sends, so
/// if it counted as a value, `required` would stop nothing a form submits.
/// `false` and `0` are values.
fn is_blank(declared: &str, value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty() && declared == "string",
        _ => false,
    }
}

/// Return `None` if the value matches the declared type; otherwise the actual
/// type as a stable string. `Null` is allowed for any type; a required field
/// rejects it separately ([`is_blank`]).
///
/// A declared type outside [`FIELD_TYPES`] passes: `/schema` no longer
/// accepts one, but boot still loads older field YAML that has one (e.g.
/// "list"), and reads of those records should not fail.
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

/// Return the offending string if `field` is a string field that declares
/// `options` and `value` is not one of them. A field without options, a
/// non-string field, and `Null` all pass.
fn invalid_option<'v>(declared: &str, options: &[&str], value: &'v Value) -> Option<&'v str> {
    let Value::String(s) = value else {
        return None;
    };
    if declared != "string" || options.is_empty() {
        return None;
    }
    if options.contains(&s.as_str()) {
        None
    } else {
        Some(s)
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
/// `projection` is as for [`check_record`]; `None` means whole records.
pub fn validate_records<'a, I>(
    schema: &VersionSchema,
    owner: &str,
    collection: &str,
    records: I,
    mode: ValidationMode,
    projection: Option<&[String]>,
) -> ValidationReport
where
    I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
{
    let mut combined = ValidationReport::default();
    for (id, fields) in records {
        let report = check_record(schema, owner, collection, fields, mode, projection);
        if !report.is_empty() {
            combined.extend(report.prefix_paths(id));
        }
    }
    combined
}

/// [`validate_record`] for an integration collection's inline fields.
///
/// `collection` is the canonical address (`east:items`,
/// `alice/sync.hub:items`), which is what the diagnostic names. The specs
/// are the type's fields or the integration's, not ordinary field documents.
pub fn validate_inline_record(
    specs: &[InlineField],
    collection: &str,
    version: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
) -> ValidationReport {
    check_inline(specs, collection, version, fields, mode, None)
}

/// [`validate_records`] for inline integration fields. `projection` is the
/// query's `fields` list; `None` means the whole record.
pub fn validate_inline_records<'a, I>(
    specs: &[InlineField],
    collection: &str,
    version: &str,
    records: I,
    mode: ValidationMode,
    projection: Option<&[String]>,
) -> ValidationReport
where
    I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
{
    let mut combined = ValidationReport::default();
    for (id, fields) in records {
        let report = check_inline(specs, collection, version, fields, mode, projection);
        if !report.is_empty() {
            combined.extend(report.prefix_paths(id));
        }
    }
    combined
}

fn check_inline(
    specs_in: &[InlineField],
    collection: &str,
    version: &str,
    fields: &HashMap<String, Value>,
    mode: ValidationMode,
    projection: Option<&[String]>,
) -> ValidationReport {
    let specs: Vec<ScalarSpec> = specs_in
        .iter()
        .map(|field| ScalarSpec {
            name: &field.name,
            ty: &field.ty,
            required: field.required,
            options: field.options.iter().map(String::as_str).collect(),
        })
        .collect();
    check_scalars(
        &specs,
        fields,
        &[],
        mode,
        Wording {
            noun: "field",
            container: &format!("collection '{collection}'"),
            version,
        },
        projection,
    )
}

/// Validate `input` against `action`'s params, the same walk and the same
/// diagnostic kinds as a `/data` create. `Ok` is the input as lake values,
/// ready for the handler. `Err` means the handler must not run.
///
/// Params stay in the order the action declares them. A JSON array or object
/// is a `type_mismatch`: the lake value is scalar only. The message says
/// `param` where a record says `field`.
pub fn validate_action_input(
    schema: &VersionSchema,
    action: &Action,
    input: &serde_json::Map<String, serde_json::Value>,
) -> Result<HashMap<String, Value>, ValidationReport> {
    let action_ref = schema.reference(action.project(), action.name());
    let specs: Vec<ScalarSpec> = action
        .params()
        .iter()
        .map(|param| ScalarSpec {
            name: param.name(),
            ty: param.r#type(),
            required: param.required,
            options: param
                .options()
                .iter()
                .map(|option| option.value())
                .collect(),
        })
        .collect();
    validate_param_specs(schema, &action_ref, &specs, input)
}

/// Validate `input` against a type action's params. `action_ref` is the
/// address the caller wrote (`sf_east:set_owner`, or qualified). The same
/// walk and diagnostic kinds as [`validate_action_input`].
pub fn validate_type_action_input(
    schema: &VersionSchema,
    action_ref: &str,
    action: &IntegrationAction,
    input: &serde_json::Map<String, serde_json::Value>,
) -> Result<HashMap<String, Value>, ValidationReport> {
    let specs = type_action_specs(action.params());
    validate_param_specs(schema, action_ref, &specs, input)
}

fn type_action_specs(params: &[IntegrationActionParam]) -> Vec<ScalarSpec<'_>> {
    params
        .iter()
        .map(|param| ScalarSpec {
            name: param.name(),
            ty: param.r#type(),
            required: param.required,
            options: param
                .options()
                .iter()
                .map(|option| option.value())
                .collect(),
        })
        .collect()
}

fn validate_param_specs(
    schema: &VersionSchema,
    action_ref: &str,
    specs: &[ScalarSpec<'_>],
    input: &serde_json::Map<String, serde_json::Value>,
) -> Result<HashMap<String, Value>, ValidationReport> {
    let mut scalars = HashMap::new();
    let mut non_scalars = Vec::new();
    for (name, value) in input {
        match coerce_scalar(value) {
            Ok(value) => {
                scalars.insert(name.clone(), value);
            }
            Err(actual) => non_scalars.push((name.clone(), actual)),
        }
    }

    let report = check_scalars(
        specs,
        &scalars,
        &non_scalars,
        ValidationMode::Create,
        Wording {
            noun: "param",
            container: &format!("action '{action_ref}'"),
            version: schema.version(),
        },
        None,
    );
    if report.has_errors() {
        Err(report)
    } else {
        Ok(scalars)
    }
}

/// Lake values are scalars. Anything else is named so a diagnostic can say
/// what arrived. A number the lake value rejects (it cannot be an i64 or an
/// f64) is reported as `number`.
fn coerce_scalar(value: &serde_json::Value) -> Result<Value, &'static str> {
    match value {
        serde_json::Value::Array(_) => Err("array"),
        serde_json::Value::Object(_) => Err("object"),
        other => serde_json::from_value(other.clone()).map_err(|_| "number"),
    }
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
        // A type written before /schema checked it should NOT trigger a
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
        assert_eq!(kind::INVALID_OPTION, "invalid_option");
        assert_eq!(kind::REQUIRED, "required");
        assert_eq!(kind::UNSUPPORTED, "unsupported");
        assert_eq!(kind::UPSTREAM, "upstream");
        assert_eq!(kind::UNAVAILABLE, "unavailable");
        assert_eq!(kind::FAILED, "failed");
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

    #[test]
    fn field_messages_keep_their_wording() {
        let specs = [ScalarSpec {
            name: "quantity",
            ty: "integer",
            required: true,
            options: Vec::new(),
        }];
        let fields = HashMap::from([("color".into(), Value::String("red".into()))]);
        let report = check_scalars(
            &specs,
            &fields,
            &[],
            ValidationMode::Create,
            Wording {
                noun: "field",
                container: "collection 'items'",
                version: "0.0.1-dev",
            },
            None,
        );
        let unknown = report
            .diagnostics
            .iter()
            .find(|d| d.kind == kind::UNKNOWN_FIELD)
            .unwrap();
        assert_eq!(
            unknown.message,
            "field 'color' is not declared in collection 'items' (version 0.0.1-dev)"
        );
        let required = report
            .diagnostics
            .iter()
            .find(|d| d.kind == kind::REQUIRED)
            .unwrap();
        assert_eq!(required.message, "field 'quantity' is required");

        let specs = [ScalarSpec {
            name: "size",
            ty: "string",
            required: true,
            options: vec!["s", "m"],
        }];
        let fields = HashMap::from([("size".into(), Value::String("xl".into()))]);
        let report = check_scalars(
            &specs,
            &fields,
            &[],
            ValidationMode::Create,
            Wording {
                noun: "field",
                container: "collection 'items'",
                version: "0.0.1-dev",
            },
            None,
        );
        assert_eq!(
            report.diagnostics[0].message,
            "field 'size' must be one of [s, m], got 'xl'"
        );
        assert_eq!(report.diagnostics[0].kind, kind::INVALID_OPTION);
    }
}
