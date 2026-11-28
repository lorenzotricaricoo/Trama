//! Bridge between the core's meeting summaries and the frontend: Tauri commands to read a
//! summary, generate it again and edit the title. Generation runs as a `summarize` job,
//! so its progress and failures arrive through the `job` event (see `jobs.rs`).

use serde::{Deserialize, Serialize};
use tauri::State;
use trama_core::jobs::lock;
use trama_core::storage::{Id, KeyPoint, Source, Summary, TitleOrigin};

use crate::jobs::{Core, JobPayload};

/// A key point as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyPointPayload {
    pub text: String,
    /// Position in the recording, in milliseconds, when known.
    pub at_ms: Option<i64>,
}

impl From<&KeyPoint> for KeyPointPayload {
    fn from(point: &KeyPoint) -> Self {
        Self {
            text: point.text.clone(),
            at_ms: point.at_ms,
        }
    }
}

/// The newest summary of a source as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryPayload {
    pub id: String,
    pub version_id: String,
    pub title: String,
    pub body: String,
    pub key_points: Vec<KeyPointPayload>,
    pub language: String,
    pub model: String,
    pub prompt_version: String,
    pub created_at: i64,
}

impl From<&Summary> for SummaryPayload {
    fn from(summary: &Summary) -> Self {
        Self {
            id: summary.id.to_string(),
            version_id: summary.version_id.to_string(),
            title: summary.title.clone(),
            body: summary.body.clone(),
            key_points: summary.key_points.iter().map(Into::into).collect(),
            language: summary.language.clone(),
            model: summary.model.clone(),
            prompt_version: summary.prompt_version.clone(),
            created_at: summary.created_at,
        }
    }
}

/// The title of a source and who gave it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TitlePayload {
    pub source_id: String,
    pub title: String,
    /// `auto` (from the summary) or `user`.
    pub title_origin: &'static str,
}

impl From<&Source> for TitlePayload {
    fn from(source: &Source) -> Self {
        Self {
            source_id: source.id.to_string(),
            title: source.title.clone(),
            title_origin: source.title_origin.as_str(),
        }
    }
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| format!("invalid source id {id:?}"))
}

/// The newest summary of the source's current version, if there is one.
#[tauri::command]
pub fn source_summary(
    core: State<'_, Core>,
    source_id: String,
) -> Result<Option<SummaryPayload>, String> {
    let summary = lock(&core.library)
        .latest_summary(parse_id(&source_id)?)
        .map_err(|err| err.to_string())?;
    Ok(summary.as_ref().map(Into::into))
}

/// Generates the summary again; the job reports its progress through the `job` event.
#[tauri::command]
pub fn regenerate_summary(core: State<'_, Core>, source_id: String) -> Result<JobPayload, String> {
    let job = trama_core::summary::regenerate(&core.runner, parse_id(&source_id)?)
        .map_err(|err| err.to_string())?;
    Ok(JobPayload::new(&job, None))
}

/// A key point as the user edited it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyPointInput {
    pub text: String,
    pub at_ms: Option<i64>,
}

/// Longest summary the user can write, in characters.
const MAX_SUMMARY_CHARS: usize = 20_000;

/// Most key points kept.
const MAX_KEY_POINTS: usize = 50;

/// Stores the user's edit of the summary and its key points. Empty key points are
/// dropped; the generated summary stays in the history.
#[tauri::command]
pub fn edit_summary(
    core: State<'_, Core>,
    source_id: String,
    body: String,
    key_points: Vec<KeyPointInput>,
) -> Result<SummaryPayload, String> {
    let body: String = body.trim().chars().take(MAX_SUMMARY_CHARS).collect();
    let points: Vec<KeyPoint> = key_points
        .iter()
        .map(|point| {
            (
                point.text.split_whitespace().collect::<Vec<_>>().join(" "),
                point.at_ms,
            )
        })
        .filter(|(text, _)| !text.is_empty())
        .take(MAX_KEY_POINTS)
        .map(|(text, at_ms)| match at_ms {
            Some(at) => KeyPoint::at(text, at.max(0)),
            None => KeyPoint::new(text),
        })
        .collect();
    let summary = lock(&core.library)
        .edit_summary(parse_id(&source_id)?, &body, &points)
        .map_err(|err| err.to_string())?;
    Ok((&summary).into())
}

/// Sets the title written by the user, which summaries never replace. An empty title
/// gives the title back to the summaries.
#[tauri::command]
pub fn set_source_title(
    core: State<'_, Core>,
    source_id: String,
    title: String,
) -> Result<TitlePayload, String> {
    let id = parse_id(&source_id)?;
    let mut library = lock(&core.library);
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let source = if title.is_empty() {
        library.reset_title(id)
    } else {
        library.set_title(id, &title, TitleOrigin::User)
    }
    .map_err(|err| err.to_string())?;
    Ok((&source).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use trama_core::storage::SourceKind;

    #[test]
    fn payloads_use_the_frontend_names() {
        let summary = Summary {
            id: Id::now_v7(),
            version_id: Id::now_v7(),
            title: "Lancio".into(),
            body: "Corpo".into(),
            key_points: vec![
                KeyPoint::at("Data fissata", 65_000),
                KeyPoint::new("Prezzo"),
            ],
            language: "it".into(),
            model: "cheap-model".into(),
            prompt_version: "summary-v1".into(),
            created_at: 7,
        };
        let json = serde_json::to_value(SummaryPayload::from(&summary)).unwrap();
        assert_eq!(json["versionId"], summary.version_id.to_string());
        assert_eq!(json["keyPoints"][0]["atMs"], 65_000);
        assert!(json["keyPoints"][1]["atMs"].is_null());
        assert_eq!(json["promptVersion"], "summary-v1");
        assert_eq!(json["createdAt"], 7);

        let source = Source {
            id: Id::now_v7(),
            kind: SourceKind::Meeting,
            title: "Mio titolo".into(),
            title_origin: TitleOrigin::User,
            language: Some("it".into()),
            occurred_at: 0,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
        };
        let json = serde_json::to_value(TitlePayload::from(&source)).unwrap();
        assert_eq!(json["sourceId"], source.id.to_string());
        assert_eq!(json["titleOrigin"], "user");
    }
}
