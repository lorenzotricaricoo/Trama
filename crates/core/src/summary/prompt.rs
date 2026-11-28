//! Prompts and reply schemas of the summary task.
//!
//! They are versioned together by [`PROMPT_VERSION`], which every stored summary records
//! with the model that wrote it: any change to a prompt, a schema or the way the input is
//! rendered needs a new version, so summaries can be compared and generated again.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::providers::JsonSchema;
use crate::storage::{Role, Segment};

/// Version of the prompts, schemas and input format below.
pub const PROMPT_VERSION: &str = "summary-v1";

/// How a transcript is written in a prompt; see [`segment_prefix`].
const TRANSCRIPT_FORMAT: &str = "The transcript has one line per stretch of speech: \
\"[HH:MM:SS] Speaker: text\". The time is from the start of the recording; \"Me\" is the \
person who recorded the meeting, \"Others\" are the other participants. The text comes from \
speech recognition and may contain recognition errors.";

/// Rules shared by every request.
const RULES: &str = "Use only what is written; never invent names, numbers, dates or \
decisions. Give each key point the time of the line where it is discussed, copied exactly \
from the input in the form HH:MM:SS.";

/// What a key point is, in every request.
const KEY_POINT: &str = "decisions, actions with their owner, open questions, important \
facts), each with \"text\", one sentence, and \"time\"";

/// The language line: one language, chosen in Settings, never detected.
fn language_rule(language: &str) -> String {
    format!(
        "Write in {} (BCP 47 tag \"{language}\"), whatever language the input is in.",
        language_name(language)
    )
}

/// English name of a language, for the prompt; the tag itself when unknown.
fn language_name(tag: &str) -> String {
    let primary = tag.split(['-', '_']).next().unwrap_or(tag).to_lowercase();
    let name = match primary.as_str() {
        "en" => "English",
        "it" => "Italian",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "ca" => "Catalan",
        "pl" => "Polish",
        "sv" => "Swedish",
        "da" => "Danish",
        "nb" | "no" => "Norwegian",
        "fi" => "Finnish",
        "ro" => "Romanian",
        "cs" => "Czech",
        "el" => "Greek",
        "ja" => "Japanese",
        "zh" => "Chinese",
        _ => return tag.to_owned(),
    };
    name.to_owned()
}

/// The request for a whole transcript that fits the model: title, summary, key points.
pub fn summary_system(language: &str) -> String {
    format!(
        "You summarize a meeting for the person who recorded it.\n{TRANSCRIPT_FORMAT}\n{}\n\
         Reply with one JSON object:\n\
         - \"title\": a specific title of at most 8 words, without date or quotes;\n\
         - \"summary\": a short summary, 2 to 5 sentences, of what was discussed and decided;\n\
         - \"key_points\": the 3 to 8 most important points ({KEY_POINT}.\n{RULES}",
        language_rule(language)
    )
}

/// The request for one part of a transcript too long for the model.
pub fn part_system(language: &str) -> String {
    format!(
        "You take notes on one part of a long meeting for the person who recorded it; the \
         notes on all the parts are combined later into one summary.\n{TRANSCRIPT_FORMAT}\n{}\n\
         Reply with one JSON object:\n\
         - \"summary\": what this part covers, up to 5 sentences;\n\
         - \"key_points\": up to 8 important points of this part ({KEY_POINT}.\n{RULES}",
        language_rule(language)
    )
}

/// How notes on parts are written in a prompt; see [`render_notes`].
const NOTES_FORMAT: &str = "Each note starts with the time range of its part, then a \
summary and key points written as \"- [HH:MM:SS] text\".";

/// The request that turns the notes on every part into the summary of the meeting.
pub fn merge_system(language: &str) -> String {
    format!(
        "You write the summary of a long meeting for the person who recorded it, from notes \
         taken on its consecutive parts.\n{NOTES_FORMAT}\n{}\n\
         Reply with one JSON object:\n\
         - \"title\": a specific title of at most 8 words, without date or quotes;\n\
         - \"summary\": a short summary, 2 to 5 sentences, of the whole meeting;\n\
         - \"key_points\": the 3 to 8 most important points of the whole meeting ({KEY_POINT}.\n\
         {RULES}",
        language_rule(language)
    )
}

/// The request that combines the notes on some consecutive parts into one note, when the
/// notes on every part are too long for one request.
pub fn combine_system(language: &str) -> String {
    format!(
        "You combine notes taken on consecutive parts of a long meeting into one note that \
         covers them all; it is combined later with the notes on the other parts.\n\
         {NOTES_FORMAT}\n{}\n\
         Reply with one JSON object:\n\
         - \"summary\": what these parts cover, up to 5 sentences;\n\
         - \"key_points\": up to 8 important points of these parts ({KEY_POINT}.\n{RULES}",
        language_rule(language)
    )
}

/// Every system prompt, to size requests for the longest.
pub fn all_systems(language: &str) -> [String; 4] {
    [
        summary_system(language),
        part_system(language),
        merge_system(language),
        combine_system(language),
    ]
}

pub fn transcript_user(transcript: &str) -> String {
    format!("Transcript of the meeting:\n\n{transcript}")
}

pub fn part_user(index: usize, count: usize, start_ms: i64, end_ms: i64, text: &str) -> String {
    format!(
        "Part {index} of {count} of the transcript, from {} to {}:\n\n{text}",
        format_time(start_ms),
        format_time(end_ms)
    )
}

pub fn notes_user(notes: &str) -> String {
    format!("Notes on the parts of the meeting, in order:\n\n{notes}")
}

/// Longest header added to the input by the user messages above, in characters.
pub const USER_HEADER_CHARS: usize = 96;

fn key_points_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["text", "time"],
            "properties": {
                "text": {"type": "string"},
                "time": {"type": "string"}
            }
        }
    })
}

/// Schema of the summary of a meeting.
pub fn summary_schema() -> JsonSchema {
    JsonSchema {
        name: "meeting_summary".into(),
        strict: true,
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["title", "summary", "key_points"],
            "properties": {
                "title": {"type": "string"},
                "summary": {"type": "string"},
                "key_points": key_points_schema()
            }
        }),
    }
}

/// Schema of the notes on one or more parts of a meeting.
pub fn notes_schema() -> JsonSchema {
    JsonSchema {
        name: "meeting_notes".into(),
        strict: true,
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["summary", "key_points"],
            "properties": {
                "summary": {"type": "string"},
                "key_points": key_points_schema()
            }
        }),
    }
}

/// A reply following [`summary_schema`].
#[derive(Debug, Deserialize)]
pub struct SummaryReply {
    pub title: String,
    pub summary: String,
    pub key_points: Vec<PointReply>,
}

/// A reply following [`notes_schema`].
#[derive(Debug, Deserialize)]
pub struct NotesReply {
    pub summary: String,
    pub key_points: Vec<PointReply>,
}

/// A key point as the model writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct PointReply {
    pub text: String,
    pub time: String,
}

/// A time in the recording as `HH:MM:SS`.
pub fn format_time(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Reads a time written by the model: `HH:MM:SS`, `H:MM:SS` or `MM:SS`, optionally in
/// brackets. `None` when it isn't one.
pub fn parse_time(text: &str) -> Option<i64> {
    let text = text
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    let parts: Vec<&str> = text.split(':').collect();
    let numbers: Option<Vec<i64>> = parts
        .iter()
        .map(|part| {
            let part = part.trim();
            (!part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect();
    let (h, m, s) = match numbers?.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => (0, *m, *s),
        _ => return None,
    };
    if s >= 60 || (parts.len() == 3 && m >= 60) {
        return None;
    }
    Some(((h * 60 + m) * 60 + s) * 1000)
}

/// How a speaker is named in a prompt.
fn speaker(role: Role) -> &'static str {
    match role {
        Role::Me => "Me",
        Role::Others => "Others",
    }
}

/// The start of a transcript line: time and speaker.
pub fn segment_prefix(segment: &Segment) -> String {
    format!(
        "[{}] {}: ",
        format_time(segment.start_ms),
        speaker(segment.role)
    )
}

/// Notes on one or more consecutive parts of a meeting, with times already checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notes {
    pub start_ms: i64,
    pub end_ms: i64,
    pub summary: String,
    /// Text and time of each key point.
    pub key_points: Vec<(String, Option<i64>)>,
}

/// Notes as written in a prompt.
pub fn render_notes(notes: &[Notes]) -> String {
    let mut out = String::new();
    for note in notes {
        out.push_str(&render_note(note));
    }
    out
}

/// One note as written in a prompt, followed by a blank line.
pub fn render_note(note: &Notes) -> String {
    let mut out = format!(
        "Part {} to {}\nSummary: {}\nKey points:\n",
        format_time(note.start_ms),
        format_time(note.end_ms),
        note.summary.trim()
    );
    for (text, at_ms) in &note.key_points {
        match at_ms {
            Some(at) => out.push_str(&format!("- [{}] {}\n", format_time(*at), text.trim())),
            None => out.push_str(&format!("- {}\n", text.trim())),
        }
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::structured::degenerate_fields;

    #[test]
    fn times_round_trip() {
        assert_eq!(format_time(0), "00:00:00");
        assert_eq!(format_time(3_725_999), "01:02:05");
        assert_eq!(parse_time("01:02:05"), Some(3_725_000));
        assert_eq!(parse_time("[00:03:15]"), Some(195_000));
        assert_eq!(parse_time(" 3:15 "), Some(195_000));
        assert_eq!(parse_time("1:00:00"), Some(3_600_000));
        for bad in ["", "soon", "00:61", "00:60:00", "1:2:3:4", "-1:00", "12"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn prompts_name_the_meeting_language() {
        for system in all_systems("it") {
            assert!(system.contains("Italian (BCP 47 tag \"it\")"), "{system}");
        }
        assert!(summary_system("pt-BR").contains("Portuguese"));
        assert!(summary_system("tlh").contains("in tlh (BCP 47 tag \"tlh\")"));
    }

    #[test]
    fn schemas_are_strict_and_reject_empty_replies() {
        for schema in [summary_schema(), notes_schema()] {
            assert!(schema.strict);
            let empty = json!({"title": "", "summary": "", "key_points": []});
            assert!(!degenerate_fields(&empty, &schema.schema).is_empty());
        }
    }

    #[test]
    fn notes_render_their_times() {
        let note = Notes {
            start_ms: 0,
            end_ms: 600_000,
            summary: "Budget".into(),
            key_points: vec![("Approved".into(), Some(65_000)), ("Later".into(), None)],
        };
        assert_eq!(
            render_note(&note),
            "Part 00:00:00 to 00:10:00\nSummary: Budget\nKey points:\n\
             - [00:01:05] Approved\n- Later\n\n"
        );
    }

    #[test]
    fn user_headers_fit_their_allowance() {
        let long = part_user(999, 999, 360_000_000, 360_000_000, "");
        assert!(long.chars().count() <= USER_HEADER_CHARS, "{long}");
        assert!(transcript_user("").chars().count() <= USER_HEADER_CHARS);
        assert!(notes_user("").chars().count() <= USER_HEADER_CHARS);
    }
}
