//! Checking structured (JSON) replies.
//!
//! Spike #12 found replies that are valid JSON and match the schema but carry nothing:
//! empty strings, zero confidence, placeholder or garbled text. They must be retried, not
//! stored (principle A4). A reply goes through three checks:
//!
//! 1. it parses as JSON (a Markdown code fence around it is tolerated);
//! 2. it matches the schema: types, required properties, enums, array items (the subset of
//!    JSON Schema the tasks use);
//! 3. it is not degenerate: no required string is empty or a placeholder, and the reply
//!    is not made only of empty strings and zeros.

use serde_json::{Map, Value};

use super::error::{ErrorKind, ProviderError};

/// Text that models write instead of content.
const PLACEHOLDERS: &[&str] = &[
    "...",
    "…",
    "n/a",
    "na",
    "null",
    "none",
    "nil",
    "undefined",
    "string",
    "text",
    "todo",
    "tbd",
    "placeholder",
    "lorem ipsum",
    "example",
];

/// Parses and checks a structured reply against `schema`.
pub fn parse_structured(content: &str, schema: &Value) -> Result<Value, ProviderError> {
    let text = strip_code_fence(content);
    if text.is_empty() {
        return Err(ProviderError::new(
            ErrorKind::EmptyReply,
            "no JSON in reply",
        ));
    }
    let value: Value = serde_json::from_str(text).map_err(|err| {
        ProviderError::new(ErrorKind::InvalidJson, format!("reply is not JSON: {err}"))
    })?;
    let mut mismatches = Vec::new();
    check_schema(&value, schema, "$", &mut mismatches);
    if !mismatches.is_empty() {
        return Err(ProviderError::new(
            ErrorKind::SchemaMismatch,
            mismatches.join("; "),
        ));
    }
    let fields = degenerate_fields(&value, schema);
    if !fields.is_empty() {
        return Err(ProviderError::new(
            ErrorKind::Degenerate {
                fields: fields.clone(),
            },
            format!("empty or placeholder fields: {}", fields.join(", ")),
        ));
    }
    Ok(value)
}

/// Removes a Markdown code fence (```json … ```) around the reply.
fn strip_code_fence(content: &str) -> &str {
    let text = content.trim();
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// Whether a string carries no content: blank, a known placeholder, a template slot such
/// as `<title>`, or no letter or digit at all.
pub fn is_placeholder(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.chars().any(char::is_alphanumeric) {
        return true;
    }
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        return true;
    }
    let lower = trimmed.to_lowercase();
    PLACEHOLDERS.contains(&lower.as_str())
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_matches(value: &Value, expected: &str) -> bool {
    let actual = type_name(value);
    actual == expected || (expected == "number" && actual == "integer")
}

fn check_schema(value: &Value, schema: &Value, path: &str, out: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    match schema.get("type") {
        Some(Value::String(expected)) if !type_matches(value, expected) => {
            out.push(format!(
                "{path}: expected {expected}, got {}",
                type_name(value)
            ));
            return;
        }
        Some(Value::Array(types))
            if !types
                .iter()
                .filter_map(Value::as_str)
                .any(|t| type_matches(value, t)) =>
        {
            out.push(format!("{path}: unexpected {}", type_name(value)));
            return;
        }
        _ => {}
    }
    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.contains(value)
    {
        out.push(format!("{path}: value not in enum"));
    }
    match value {
        Value::Object(object) => check_object(object, schema, path, out),
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    check_schema(item, item_schema, &format!("{path}[{i}]"), out);
                }
            }
        }
        _ => {}
    }
}

fn check_object(
    object: &Map<String, Value>,
    schema: &Map<String, Value>,
    path: &str,
    out: &mut Vec<String>,
) {
    for name in required(schema) {
        if !object.contains_key(name) {
            out.push(format!("{path}.{name}: missing"));
        }
    }
    if let Some(Value::Object(properties)) = schema.get("properties") {
        for (name, property) in properties {
            if let Some(child) = object.get(name) {
                check_schema(child, property, &format!("{path}.{name}"), out);
            }
        }
    }
}

fn required(schema: &Map<String, Value>) -> impl Iterator<Item = &str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// Paths of the fields that make a schema-valid reply degenerate; empty if it is fine.
pub fn degenerate_fields(value: &Value, schema: &Value) -> Vec<String> {
    let mut fields = Vec::new();
    required_placeholders(value, schema, "$", true, &mut fields);
    if fields.is_empty() {
        let mut leaves = Leaves::default();
        leaves.visit(value, schema);
        if leaves.any && !leaves.meaningful {
            fields.push("$".to_owned());
        }
    }
    fields
}

/// Collects required strings (and strings inside required arrays) that are placeholders.
fn required_placeholders(
    value: &Value,
    schema: &Value,
    path: &str,
    is_required: bool,
    out: &mut Vec<String>,
) {
    let schema = schema.as_object();
    // A value the schema lists explicitly (an enum such as "none") is never a placeholder.
    let allowed = schema
        .and_then(|s| s.get("enum"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.contains(value));
    match value {
        Value::String(text) if is_required && !allowed && is_placeholder(text) => {
            out.push(path.to_owned());
        }
        Value::Object(object) => {
            let Some(schema) = schema else { return };
            let required: Vec<&str> = required(schema).collect();
            let properties = schema.get("properties").and_then(Value::as_object);
            for (name, child) in object {
                let child_schema = properties.and_then(|p| p.get(name)).unwrap_or(&Value::Null);
                required_placeholders(
                    child,
                    child_schema,
                    &format!("{path}.{name}"),
                    required.contains(&name.as_str()),
                    out,
                );
            }
        }
        Value::Array(items) => {
            let item_schema = schema.and_then(|s| s.get("items")).unwrap_or(&Value::Null);
            for (i, item) in items.iter().enumerate() {
                // An item of a required array must carry content too.
                required_placeholders(item, item_schema, &format!("{path}[{i}]"), is_required, out);
            }
        }
        _ => {}
    }
}

/// Whether a reply has any string or number, and whether any of them carries content.
#[derive(Default)]
struct Leaves {
    any: bool,
    meaningful: bool,
}

impl Leaves {
    fn visit(&mut self, value: &Value, schema: &Value) {
        let schema = schema.as_object();
        let child_schema = |key: &str, name: &str| {
            schema
                .and_then(|s| s.get(key))
                .and_then(|v| {
                    if name.is_empty() {
                        Some(v)
                    } else {
                        v.get(name)
                    }
                })
                .unwrap_or(&Value::Null)
        };
        match value {
            Value::String(text) => {
                self.any = true;
                let listed = child_schema("enum", "")
                    .as_array()
                    .is_some_and(|values| values.contains(value));
                self.meaningful |= listed || !is_placeholder(text);
            }
            Value::Number(n) => {
                self.any = true;
                self.meaningful |= n.as_f64().is_some_and(|f| f != 0.0);
            }
            Value::Bool(_) => {
                self.any = true;
                self.meaningful = true;
            }
            Value::Array(items) => {
                let item_schema = child_schema("items", "");
                items.iter().for_each(|item| self.visit(item, item_schema));
            }
            Value::Object(object) => object
                .iter()
                .for_each(|(name, child)| self.visit(child, child_schema("properties", name))),
            Value::Null => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn summary_schema() -> Value {
        json!({
            "type": "object",
            "required": ["title", "body", "key_points"],
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"},
                "key_points": {"type": "array", "items": {"type": "string"}},
                "confidence": {"type": "number"}
            }
        })
    }

    #[test]
    fn accepts_a_good_reply_with_or_without_a_code_fence() {
        let good = r#"{"title":"Kick-off","body":"We agreed.","key_points":["Ship in May"]}"#;
        assert!(parse_structured(good, &summary_schema()).is_ok());
        let fenced = format!("```json\n{good}\n```");
        assert!(parse_structured(&fenced, &summary_schema()).is_ok());
    }

    #[test]
    fn empty_and_non_json_replies_are_errors() {
        let err = parse_structured("  ", &summary_schema()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::EmptyReply);
        let err = parse_structured("Sure! Here is", &summary_schema()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidJson);
    }

    #[test]
    fn schema_mismatches_are_reported() {
        let missing = r#"{"title":"T","body":"B"}"#;
        let err = parse_structured(missing, &summary_schema()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::SchemaMismatch);
        assert!(err.detail.contains("key_points"));

        let wrong_type = r#"{"title":"T","body":"B","key_points":"one"}"#;
        let err = parse_structured(wrong_type, &summary_schema()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::SchemaMismatch);

        let schema = json!({"type":"object","properties":{"status":{"enum":["open","done"]}}});
        let err = parse_structured(r#"{"status":"maybe"}"#, &schema).unwrap_err();
        assert_eq!(err.kind, ErrorKind::SchemaMismatch);
    }

    #[test]
    fn schema_valid_but_empty_replies_are_degenerate() {
        let empty = r#"{"title":"","body":"   ","key_points":[],"confidence":0.0}"#;
        let err = parse_structured(empty, &summary_schema()).unwrap_err();
        let ErrorKind::Degenerate { fields } = err.kind else {
            panic!("expected degenerate, got {err:?}");
        };
        assert_eq!(fields, ["$.body", "$.title"]);
    }

    #[test]
    fn placeholders_and_garbled_text_are_degenerate() {
        for bad in ["...", "N/A", "<title>", "}}}", "string", "—"] {
            let reply = json!({"title": bad, "body": "Real text", "key_points": ["a point"]});
            let fields = degenerate_fields(&reply, &summary_schema());
            assert_eq!(fields, ["$.title"], "{bad}");
        }
        let reply = json!({"title": "T", "body": "B", "key_points": ["ok", ""]});
        assert_eq!(
            degenerate_fields(&reply, &summary_schema()),
            ["$.key_points[1]"]
        );
    }

    #[test]
    fn enum_values_are_never_placeholders() {
        let schema = json!({
            "type": "object",
            "required": ["owner"],
            "properties": {"owner": {"type": "string", "enum": ["none", "me", "others"]}}
        });
        assert!(degenerate_fields(&json!({"owner": "none"}), &schema).is_empty());
    }

    #[test]
    fn nested_required_fields_are_checked() {
        let schema = json!({
            "type": "object",
            "required": ["people"],
            "properties": {"people": {"type": "array", "items": {
                "type": "object",
                "required": ["name", "evidence"],
                "properties": {"name": {"type": "string"}, "evidence": {"type": "string"},
                               "note": {"type": "string"}}
            }}}
        });
        let reply = json!({"people": [
            {"name": "Anna", "evidence": "Anna said so", "note": ""},
            {"name": "", "evidence": "x said"}
        ]});
        assert_eq!(degenerate_fields(&reply, &schema), ["$.people[1].name"]);
    }

    #[test]
    fn all_empty_replies_are_degenerate_even_without_required_fields() {
        let schema = json!({"type": "object"});
        assert_eq!(
            degenerate_fields(&json!({"reason": "", "confidence": 0}), &schema),
            ["$"]
        );
        // Nothing found is a legitimate answer, not a degenerate one.
        assert!(degenerate_fields(&json!({"people": [], "decisions": []}), &schema).is_empty());
        assert!(
            degenerate_fields(&json!({"project": null, "confidence": 0.4}), &schema).is_empty()
        );
    }
}
