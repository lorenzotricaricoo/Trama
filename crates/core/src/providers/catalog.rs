//! Reading a provider's model catalogue (`GET /models`).
//!
//! The OpenAI format lists only model ids; many compatible providers add the limits, each
//! under its own field names. The known names are tried in order; a limit nobody reports
//! stays unknown and the user can set it by hand (principle A4: never hard-coded).

use serde_json::{Map, Value};

use super::error::{ErrorKind, ProviderError};
use crate::storage::{CatalogEntry, ModelLimits};

/// Fields that hold the context window, in order of preference.
const CONTEXT_FIELDS: &[&str] = &[
    "context_length",
    "context_window",
    "max_context_length",
    "max_model_len",
    "context_tokens",
    "max_input_tokens",
    "input_token_limit",
    "inputTokenLimit",
];

/// Fields that hold the longest reply, in order of preference.
const OUTPUT_FIELDS: &[&str] = &[
    "max_output_tokens",
    "max_completion_tokens",
    "output_token_limit",
    "outputTokenLimit",
    "max_output",
    "max_tokens",
];

/// Nested objects where some providers put the limits.
const NESTED: &[&str] = &[
    "top_provider",
    "limits",
    "limit",
    "metadata",
    "model_info",
    "capabilities",
    "info",
];

/// Parses a `/models` reply: `{"data": [...]}` or a bare array, each entry with an `id`.
pub fn parse_catalog(body: &str) -> Result<Vec<CatalogEntry>, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|err| {
        ProviderError::new(
            ErrorKind::InvalidResponse,
            format!("model list is not JSON: {err}"),
        )
    })?;
    let models = match &value {
        Value::Array(models) => models,
        Value::Object(object) => match object.get("data").or_else(|| object.get("models")) {
            Some(Value::Array(models)) => models,
            _ => {
                return Err(ProviderError::new(
                    ErrorKind::InvalidResponse,
                    "model list has no data array",
                ));
            }
        },
        _ => {
            return Err(ProviderError::new(
                ErrorKind::InvalidResponse,
                "model list is neither an object nor an array",
            ));
        }
    };
    let mut entries: Vec<CatalogEntry> = models
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|model| {
            let id = model
                .get("id")
                .or_else(|| model.get("name"))
                .and_then(Value::as_str)?
                .trim();
            (!id.is_empty()).then(|| CatalogEntry {
                model_id: id.to_owned(),
                limits: ModelLimits {
                    context_tokens: find_limit(model, CONTEXT_FIELDS),
                    max_output_tokens: find_limit(model, OUTPUT_FIELDS),
                },
            })
        })
        .collect();
    entries.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    entries.dedup_by(|a, b| a.model_id == b.model_id);
    Ok(entries)
}

/// The first positive limit under one of `fields`, at the top level or in a nested object.
fn find_limit(model: &Map<String, Value>, fields: &[&str]) -> Option<u32> {
    let direct = fields.iter().find_map(|f| model.get(*f).and_then(as_limit));
    direct.or_else(|| {
        NESTED
            .iter()
            .filter_map(|nested| model.get(*nested).and_then(Value::as_object))
            .find_map(|nested| {
                fields
                    .iter()
                    .find_map(|f| nested.get(*f).and_then(as_limit))
            })
    })
}

/// A positive token count, from a number or a numeric string.
fn as_limit(value: &Value) -> Option<u32> {
    let n = match value {
        Value::Number(n) => n.as_u64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite() && *f >= 1.0)
                .map(|f| f as u64)
        })?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (n > 0).then(|| u32::try_from(n).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(context: Option<u32>, output: Option<u32>) -> ModelLimits {
        ModelLimits {
            context_tokens: context,
            max_output_tokens: output,
        }
    }

    #[test]
    fn plain_openai_list_has_no_limits() {
        let body = r#"{"object":"list","data":[
            {"id":"b-model","object":"model","owned_by":"x"},
            {"id":"a-model","object":"model"}]}"#;
        let entries = parse_catalog(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].model_id, "a-model");
        assert_eq!(entries[0].limits, ModelLimits::default());
    }

    #[test]
    fn reads_limits_under_known_names() {
        let body = r#"{"data":[
            {"id":"router","context_length":200000,"top_provider":{"max_completion_tokens":8192}},
            {"id":"groq","context_window":131072,"max_completion_tokens":"32768"},
            {"id":"mistral","max_context_length":32768},
            {"id":"vllm","max_model_len":4096},
            {"id":"anthropic","max_input_tokens":200000,"max_tokens":64000},
            {"id":"gemini","inputTokenLimit":1048576,"outputTokenLimit":65536},
            {"id":"nested","limits":{"context_window":1000,"max_output_tokens":100}},
            {"id":"zero","context_length":0,"max_output_tokens":-1}
        ]}"#;
        let entries = parse_catalog(body).unwrap();
        let get = |id: &str| entries.iter().find(|e| e.model_id == id).unwrap().limits;
        assert_eq!(get("router"), limits(Some(200_000), Some(8192)));
        assert_eq!(get("groq"), limits(Some(131_072), Some(32_768)));
        assert_eq!(get("mistral"), limits(Some(32_768), None));
        assert_eq!(get("vllm"), limits(Some(4096), None));
        assert_eq!(get("anthropic"), limits(Some(200_000), Some(64_000)));
        assert_eq!(get("gemini"), limits(Some(1_048_576), Some(65_536)));
        assert_eq!(get("nested"), limits(Some(1000), Some(100)));
        assert_eq!(get("zero"), limits(None, None));
    }

    #[test]
    fn accepts_a_bare_array_and_skips_entries_without_id() {
        let body = r#"[{"id":"one"},{"object":"model"},{"id":"  "},{"id":"one"}]"#;
        let entries = parse_catalog(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model_id, "one");
    }

    #[test]
    fn refuses_what_is_not_a_model_list() {
        for body in ["not json", r#"{"error":"nope"}"#, "42"] {
            let err = parse_catalog(body).unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidResponse, "{body}");
        }
    }
}
