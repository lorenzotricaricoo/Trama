//! Meeting summaries: a title, a short summary and key points with timestamps, written
//! by the model the user chose for the summary task (see `docs/architecture/providers.md`).
//!
//! - The summary is written in the meeting's language, chosen in Settings
//!   ([`Library::meeting_language`]); it is never detected.
//! - A transcript too long for the model's context window is split into parts that fit;
//!   notes on each part are then merged into one summary, in more rounds if the notes
//!   don't fit one request either. Sizes are estimated pessimistically and a fifth of the
//!   room is kept free; if the provider still finds a request too long, the work starts
//!   again once with parts half the size. Nothing is truncated: if even that is
//!   impossible the job fails (`context_exceeded`, `bad_request`).
//! - Prompts and schemas are versioned ([`PROMPT_VERSION`]); each summary records it with
//!   the model.
//! - Generating again adds a summary; a title written by the user is never replaced.
//!
//! The work runs as a [`JobKind::Summarize`] job ([`Summarize`]): failures are job errors
//! with a code the interface translates, retried with backoff when that may help. After
//! the last attempt the meeting is ready anyway (the summary is optional) and the failed
//! job stays visible for the user to retry; a cancelled job stores nothing.

mod prompt;
mod split;

use std::sync::Arc;

use serde::de::DeserializeOwned;

pub use prompt::PROMPT_VERSION;

use crate::i18n::Catalog;
use crate::jobs::{JobContext, JobError, JobHandler, Runner};
use crate::providers::{
    self, ClientConfig, ErrorKind, JsonSchema, Message, ProviderError, SecretStore,
    TaskModelClient, estimate_tokens,
};
use crate::storage::{
    Id, Job, JobKind, JobState, KeyPoint, Library, MeetingStatus, ModelLimits, ModelTask,
    NewSummary, Segment, SourceKind, StorageError, Summary, VersionOrigin,
};
use prompt::{Notes, PointReply};

/// Error codes of the summary job, translated in `locales/<locale>/summary.ftl` as
/// `summary-error-<code>` (with `_` written as `-`). Errors of the provider call keep the
/// provider codes ([`providers::error_code`]).
pub mod error_code {
    /// Summaries are only written for meetings.
    pub const UNSUPPORTED_SOURCE: &str = "unsupported_source";
    /// The meeting has no transcription yet.
    pub const NO_TRANSCRIPT: &str = "no_transcript";
    /// The transcription has no words: nothing to summarize.
    pub const EMPTY_TRANSCRIPT: &str = "empty_transcript";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[UNSUPPORTED_SOURCE, NO_TRANSCRIPT, EMPTY_TRANSCRIPT];
}

/// Messages for the summary error codes, one catalog per supported locale.
pub const CATALOGS: &[Catalog] = &[
    Catalog {
        locale: "en",
        name: "summary",
        source: include_str!("../../../../locales/en/summary.ftl"),
    },
    Catalog {
        locale: "it",
        name: "summary",
        source: include_str!("../../../../locales/it/summary.ftl"),
    },
];

/// Message id of an error code in [`CATALOGS`], for example `summary-error-no-transcript`.
pub fn error_message_id(code: &str) -> String {
    format!("summary-error-{}", code.replace('_', "-"))
}

/// Longest reply asked for, in tokens; less when the model's output limit, or a quarter of
/// its context window, is smaller.
pub const SUMMARY_OUTPUT_TOKENS: u32 = 2048;

/// Smallest room for transcript text in one request, in tokens. A model with less room is
/// refused (`context_exceeded`) rather than asked hundreds of times.
pub const MIN_INPUT_TOKENS: u32 = 1024;

/// Tokens kept free in each request for the message framing and for the instruction and
/// schema the client adds to the prompt when a provider refuses JSON schemas.
const PROMPT_ALLOWANCE_TOKENS: u32 = 160;

/// Share of the room for text left unused, in percent: token counts are estimates, and a
/// request that turns out too long costs a whole round of calls.
pub const INPUT_RESERVE_PERCENT: u32 = 20;

/// Queues the summary of a meeting whose transcription was just stored. The transcription
/// job calls this once it has stored the new version (`Library::complete_transcription`).
pub fn enqueue_after_transcription(ctx: &JobContext, meeting_id: Id) -> Result<Job, StorageError> {
    ctx.enqueue(JobKind::Summarize, meeting_id)
}

/// Generates the summary of a source again, on the user's request. If a summary job is
/// already queued or running for it, returns that one.
pub fn regenerate(runner: &Runner, source_id: Id) -> Result<Job, StorageError> {
    runner.enqueue(JobKind::Summarize, source_id)
}

/// Handler for [`JobKind::Summarize`].
pub struct Summarize {
    secrets: Arc<dyn SecretStore>,
    config: ClientConfig,
}

impl std::fmt::Debug for Summarize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Summarize")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// What a summary is written from, read from the library before any provider call.
struct Prepared {
    version_id: Id,
    language: String,
    segments: Vec<Segment>,
    model: TaskModelClient,
}

impl Summarize {
    /// A handler that reads API keys from `secrets` (the Keychain in the app).
    pub fn new(secrets: Arc<dyn SecretStore>, config: ClientConfig) -> Self {
        Self { secrets, config }
    }

    /// Summarizes the current transcription of a meeting and stores the summary, holding
    /// the library throughout. The job does the same without holding it during the
    /// provider calls.
    pub fn summarize_now(&self, library: &mut Library, source_id: Id) -> Result<Summary, JobError> {
        let prepared = self.prepare(library, source_id)?;
        let summary = generate(&prepared, &|_| {}, &|| false)?;
        store(library, source_id, &prepared, &summary)
    }

    fn prepare(&self, library: &Library, source_id: Id) -> Result<Prepared, JobError> {
        let source = library.source(source_id)?;
        if source.kind != SourceKind::Meeting {
            return Err(JobError::permanent(
                error_code::UNSUPPORTED_SOURCE,
                format!("source {source_id} is a {}", source.kind.as_str()),
            ));
        }
        let version = library
            .current_version(source_id)?
            .filter(|v| v.origin == VersionOrigin::Transcription)
            .ok_or_else(|| {
                JobError::permanent(
                    error_code::NO_TRANSCRIPT,
                    format!("meeting {source_id} has no transcription"),
                )
            })?;
        let segments: Vec<Segment> = library
            .segments(version.id)?
            .into_iter()
            .filter(|s| !s.text.trim().is_empty())
            .collect();
        if segments.is_empty() {
            return Err(JobError::permanent(
                error_code::EMPTY_TRANSCRIPT,
                format!("version {} has no words", version.id),
            ));
        }
        let language = match source.language.filter(|tag| !tag.trim().is_empty()) {
            Some(tag) => tag,
            None => library.meeting_language()?,
        };
        let model = providers::task_client(
            library,
            self.secrets.as_ref(),
            ModelTask::Summary,
            self.config.clone(),
        )?;
        Ok(Prepared {
            version_id: version.id,
            language,
            segments,
            model,
        })
    }
}

impl JobHandler for Summarize {
    fn kind(&self) -> JobKind {
        JobKind::Summarize
    }

    fn run(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        let result = self.attempt(job, ctx);
        if let Err(err) = &result
            && !ctx.should_stop()
            && (!err.retryable || job.attempts >= job.max_attempts)
        {
            // This was the last attempt. The transcript is complete and the summary is
            // optional: the meeting is ready, and the failed job stays visible with its
            // code until the user retries it.
            if let Err(storage) = mark_ready(&mut ctx.library(), job.target_id) {
                eprintln!(
                    "trama: cannot mark meeting {} as ready: {storage}",
                    job.target_id
                );
            }
        }
        result
    }
}

impl Summarize {
    fn attempt(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        let mut prepared = {
            let library = ctx.library();
            self.prepare(&library, job.target_id)?
        };
        // Each call counts towards the month's spend and is checked against the budget.
        prepared.model = prepared.model.recording_usage(ctx.shared_library());
        // The library stays free during the provider calls.
        let summary = generate(&prepared, &|f| ctx.progress(f), &|| ctx.should_stop())?;
        let mut library = ctx.library();
        // Cancelled or stopping during the last call: write nothing. The stored state
        // covers a cancellation stored just before its flag is raised.
        if ctx.should_stop() || library.job(job.id)?.state != JobState::Running {
            return Err(JobError::stopped());
        }
        store(&mut library, job.target_id, &prepared, &summary)?;
        drop(library);
        ctx.progress(1.0);
        Ok(())
    }
}

/// Stores the summary (it titles the meeting unless the user did) and marks a meeting
/// that was waiting for it as ready.
fn store(
    library: &mut Library,
    source_id: Id,
    prepared: &Prepared,
    summary: &NewSummary,
) -> Result<Summary, JobError> {
    let stored = library.add_summary(prepared.version_id, summary)?;
    mark_ready(library, source_id)?;
    Ok(stored)
}

/// Moves a meeting waiting for its summary (`processing`) to `ready`.
fn mark_ready(library: &mut Library, source_id: Id) -> Result<(), StorageError> {
    let source = library.source(source_id)?;
    if source.kind == SourceKind::Meeting
        && source.deleted_at.is_none()
        && library.meeting(source_id)?.status == MeetingStatus::Processing
    {
        library.set_meeting_status(source_id, MeetingStatus::Ready, None)?;
    }
    Ok(())
}

/// Sizes of one request, in tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Budget {
    /// Longest reply asked for.
    output: u32,
    /// Room for the transcript or the notes.
    input: u32,
}

impl Budget {
    fn new(limits: ModelLimits, language: &str) -> Result<Self, ProviderError> {
        let context = limits.context_tokens.ok_or_else(|| {
            ProviderError::new(
                ErrorKind::LimitsUnknown,
                "context window of the model unknown",
            )
        })?;
        let output = SUMMARY_OUTPUT_TOKENS
            .min(limits.max_output_tokens.unwrap_or(u32::MAX))
            .min(context / 4)
            .max(1);
        let system = prompt::all_systems(language)
            .iter()
            .map(|s| estimate_tokens(s))
            .max()
            .unwrap_or(0);
        let schema = [prompt::summary_schema(), prompt::notes_schema()]
            .iter()
            .map(|s| estimate_tokens(&s.schema.to_string()))
            .max()
            .unwrap_or(0);
        let header = u32::try_from(prompt::USER_HEADER_CHARS.div_ceil(3)).unwrap_or(u32::MAX);
        let overhead = system
            .saturating_add(schema)
            .saturating_add(header)
            .saturating_add(PROMPT_ALLOWANCE_TOKENS);
        let room = context.saturating_sub(output).saturating_sub(overhead);
        let input = room - room * INPUT_RESERVE_PERCENT / 100;
        if input < MIN_INPUT_TOKENS.max(output) {
            return Err(ProviderError::new(
                ErrorKind::ContextExceeded,
                format!(
                    "a context of {context} tokens leaves {input} for the transcript; \
                     summaries need at least {}",
                    MIN_INPUT_TOKENS.max(output)
                ),
            ));
        }
        Ok(Self { output, input })
    }

    /// Room for the input, for the splitter.
    fn input_tokens(self) -> usize {
        usize::try_from(self.input).unwrap_or(usize::MAX)
    }

    /// The same request with half the room for text, after the provider found a request
    /// too long.
    fn halved(self) -> Self {
        Self {
            input: (self.input / 2).max(1),
            ..self
        }
    }
}

/// Whether the provider refused a request because it was too long for the model (HTTP
/// 413, or a 400 or 422 that says so): worth one more try with smaller parts.
fn is_too_long(err: &JobError) -> bool {
    if err.code != providers::error_code::BAD_REQUEST {
        return false;
    }
    let detail = err.detail.to_lowercase();
    detail.starts_with("http 413")
        || [
            "context",
            "too long",
            "too large",
            "too many tokens",
            "token limit",
            "maximum",
        ]
        .iter()
        .any(|hint| detail.contains(hint))
}

/// Writes the summary. If the provider finds a request too long despite the estimates,
/// starts again once with parts half the size.
fn generate(
    prepared: &Prepared,
    progress: &dyn Fn(f32),
    should_stop: &dyn Fn() -> bool,
) -> Result<NewSummary, JobError> {
    let budget = Budget::new(prepared.model.limits(), &prepared.language)?;
    match generate_within(prepared, budget, progress, should_stop) {
        Err(err) if is_too_long(&err) => {
            generate_within(prepared, budget.halved(), progress, should_stop)
        }
        result => result,
    }
}

/// Writes the summary within `budget`: in one request if the transcript fits, otherwise
/// part by part and then merged.
fn generate_within(
    prepared: &Prepared,
    budget: Budget,
    progress: &dyn Fn(f32),
    should_stop: &dyn Fn() -> bool,
) -> Result<NewSummary, JobError> {
    let language = prepared.language.as_str();
    let sizer = split::Sizer::for_language(language);
    let max_tokens = budget.input_tokens();
    let parts = split::parts(&prepared.segments, max_tokens, sizer);
    let points = |points: Vec<PointReply>| key_points(points, &prepared.segments);
    let stop = || {
        if should_stop() {
            Err(JobError::stopped())
        } else {
            Ok(())
        }
    };

    if let [part] = parts.as_slice() {
        stop()?;
        let reply: prompt::SummaryReply = ask(
            &prepared.model,
            prompt::summary_system(language),
            prompt::transcript_user(&part.text),
            &prompt::summary_schema(),
            budget.output,
        )?;
        return Ok(new_summary(prepared, reply, points));
    }

    // One request per part, one to merge, and maybe a few more to combine notes.
    let total = parts.len() as f32 + 1.0;
    let mut done = 0.0;
    let mut step = || {
        done += 1.0;
        progress((done / total).min(0.95));
    };
    let mut notes = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        stop()?;
        let reply: prompt::NotesReply = ask(
            &prepared.model,
            prompt::part_system(language),
            prompt::part_user(
                index + 1,
                parts.len(),
                part.start_ms,
                part.end_ms,
                &part.text,
            ),
            &prompt::notes_schema(),
            budget.output,
        )?;
        notes.push(Notes {
            start_ms: part.start_ms,
            end_ms: part.end_ms,
            summary: reply.summary.trim().to_owned(),
            key_points: points(reply.key_points)
                .into_iter()
                .map(|p| (p.text, p.at_ms))
                .collect(),
        });
        step();
    }

    loop {
        stop()?;
        let rendered = prompt::render_notes(&notes);
        if split::notes_tokens(&notes, sizer) <= max_tokens {
            let reply: prompt::SummaryReply = ask(
                &prepared.model,
                prompt::merge_system(language),
                prompt::notes_user(&rendered),
                &prompt::summary_schema(),
                budget.output,
            )?;
            return Ok(new_summary(prepared, reply, points));
        }
        // The notes don't fit one request: combine neighbours until they do.
        let groups = split::group_notes(&notes, max_tokens, sizer);
        if groups.len() == notes.len() {
            return Err(ProviderError::new(
                ErrorKind::ContextExceeded,
                format!(
                    "{} notes of about {} tokens can't be combined within the context",
                    notes.len(),
                    estimate_tokens(&rendered)
                ),
            )
            .into());
        }
        let mut combined = Vec::with_capacity(groups.len());
        for group in groups {
            let group = &notes[group];
            let (Some(first), Some(last)) = (group.first(), group.last()) else {
                continue;
            };
            if group.len() == 1 {
                combined.push(first.clone());
                continue;
            }
            stop()?;
            let reply: prompt::NotesReply = ask(
                &prepared.model,
                prompt::combine_system(language),
                prompt::notes_user(&prompt::render_notes(group)),
                &prompt::notes_schema(),
                budget.output,
            )?;
            combined.push(Notes {
                start_ms: first.start_ms,
                end_ms: last.end_ms,
                summary: reply.summary.trim().to_owned(),
                key_points: points(reply.key_points)
                    .into_iter()
                    .map(|p| (p.text, p.at_ms))
                    .collect(),
            });
        }
        notes = combined;
    }
}

/// One structured request to the summary model.
fn ask<T: DeserializeOwned>(
    model: &TaskModelClient,
    system: String,
    user: String,
    schema: &JsonSchema,
    max_output_tokens: u32,
) -> Result<T, JobError> {
    let reply = model.chat_structured(
        vec![Message::system(system), Message::user(user)],
        schema,
        Some(max_output_tokens),
    )?;
    serde_json::from_value(reply.value).map_err(|err| {
        JobError::from(ProviderError::new(
            ErrorKind::SchemaMismatch,
            err.to_string(),
        ))
    })
}

/// Key points as stored: trimmed text, and the start of the segment nearest to the time
/// the model gave (none if it gave no time it can read).
fn key_points(points: Vec<PointReply>, segments: &[Segment]) -> Vec<KeyPoint> {
    points
        .into_iter()
        .map(|point| KeyPoint {
            text: point.text.trim().to_owned(),
            at_ms: prompt::parse_time(&point.time).and_then(|at| split::snap(at, segments)),
        })
        .filter(|point| !point.text.is_empty())
        .collect()
}

fn new_summary(
    prepared: &Prepared,
    reply: prompt::SummaryReply,
    points: impl Fn(Vec<PointReply>) -> Vec<KeyPoint>,
) -> NewSummary {
    NewSummary {
        title: clean_title(&reply.title),
        body: reply.summary.trim().to_owned(),
        key_points: points(reply.key_points),
        language: prepared.language.clone(),
        model: prepared.model.model.model_id.clone(),
        prompt_version: PROMPT_VERSION.to_owned(),
    }
}

/// A title on one line, without the quotes models like to add.
fn clean_title(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let quotes = [('"', '"'), ('\'', '\''), ('“', '”'), ('«', '»'), ('*', '*')];
    let mut title = title.as_str();
    while let Some((open, close)) = quotes.iter().find(|(open, close)| {
        title.chars().count() > 2 && title.starts_with(*open) && title.ends_with(*close)
    }) {
        title = title[open.len_utf8()..title.len() - close.len_utf8()].trim();
    }
    title.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Localizer, SUPPORTED_LOCALES};

    #[test]
    fn every_error_code_is_translated() {
        for locale in SUPPORTED_LOCALES {
            let locale = locale.to_string();
            let l10n = Localizer::new(&[locale.as_str()], CATALOGS).unwrap();
            for code in error_code::ALL {
                let id = error_message_id(code);
                assert_ne!(l10n.message(&id, None), id, "{id} missing in {locale}");
            }
        }
    }

    #[test]
    fn titles_lose_quotes_and_line_breaks() {
        assert_eq!(clean_title("  \"Budget 2027\"  "), "Budget 2027");
        assert_eq!(clean_title("«**Kick-off**»"), "Kick-off");
        assert_eq!(clean_title("Piano\n trimestrale"), "Piano trimestrale");
        assert_eq!(clean_title("\"\""), "\"\"");
    }

    #[test]
    fn budgets_follow_the_model_limits() {
        let limits = |context, output| ModelLimits {
            context_tokens: Some(context),
            max_output_tokens: output,
        };
        let big = Budget::new(limits(200_000, Some(8_000)), "it").unwrap();
        assert_eq!(big.output, SUMMARY_OUTPUT_TOKENS);
        // A fifth of the room for text stays unused, as a margin on the estimates.
        assert!(
            big.input > 150_000 && big.input <= 200_000 * 4 / 5,
            "{big:?}"
        );
        assert_eq!(big.halved().input, big.input / 2);
        assert_eq!(big.halved().output, big.output);

        let small = Budget::new(limits(8_000, None), "it").unwrap();
        assert_eq!(small.output, 2_000);
        assert!(small.input + small.output < 8_000);

        let tight = Budget::new(limits(16_000, Some(500)), "it").unwrap();
        assert_eq!(tight.output, 500);

        let err = Budget::new(limits(2_000, None), "it").unwrap_err();
        assert_eq!(err.kind, ErrorKind::ContextExceeded);
        let err = Budget::new(ModelLimits::default(), "it").unwrap_err();
        assert_eq!(err.kind, ErrorKind::LimitsUnknown);
    }

    #[test]
    fn requests_too_long_for_the_provider_are_recognized() {
        let bad = |detail: &str| JobError::permanent("bad_request", detail);
        for detail in [
            "HTTP 413: payload",
            "HTTP 400: {\"error\":{\"message\":\"This model's maximum context length is 8192\"}}",
            "HTTP 400: prompt is too long: 9000 tokens > 8192",
            "HTTP 422: input exceeds the token limit",
        ] {
            assert!(is_too_long(&bad(detail)), "{detail}");
        }
        assert!(!is_too_long(&bad(
            "HTTP 400: unknown parameter response_format"
        )));
        assert!(!is_too_long(&JobError::permanent(
            "context_exceeded",
            "notes can't be combined within the context"
        )));
        assert!(!is_too_long(&JobError::permanent(
            "auth_failed",
            "HTTP 401: too long"
        )));
    }
}
