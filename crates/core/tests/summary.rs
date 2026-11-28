//! Meeting summaries against a mock provider on localhost: synthetic transcripts only, no
//! real provider is called.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::http::{MockServer, Recorded, Reply};
use common::{ManualClock, library};
use serde_json::json;
use tempfile::TempDir;
use trama_core::jobs::{EventSink, JobContext, JobError, JobEvent, JobHandler, Runner, lock};
use trama_core::providers::{self, ClientConfig, MemorySecrets, RetryPolicy, Secret};
use trama_core::storage::*;
use trama_core::summary::{self, PROMPT_VERSION, Summarize};

const KEY: &str = "sk-test-summary-0123";
const MODEL: &str = "cheap-model";
const PATIENCE: Duration = Duration::from_secs(10);

fn config() -> ClientConfig {
    ClientConfig {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        retry: RetryPolicy {
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            ..RetryPolicy::default()
        },
    }
}

/// A library whose summary task uses `MODEL` on the mock server, with these limits.
fn configured(
    server: &MockServer,
    context: u32,
    output: Option<u32>,
) -> (TempDir, ManualClock, Library, Summarize) {
    let (dir, clock, mut library) = library();
    let secrets = Arc::new(MemorySecrets::new());
    let provider = providers::add_provider(
        &mut library,
        secrets.as_ref(),
        &NewProvider {
            name: "Mock".into(),
            base_url: server.url(),
            needs_key: true,
        },
        Some(&Secret::new(KEY)),
    )
    .unwrap();
    library
        .replace_catalog(
            provider.id,
            &[CatalogEntry {
                model_id: MODEL.into(),
                limits: ModelLimits {
                    context_tokens: Some(context),
                    max_output_tokens: output,
                },
            }],
        )
        .unwrap();
    library
        .set_task_model(ModelTask::Summary, provider.id, MODEL)
        .unwrap();
    (dir, clock, library, Summarize::new(secrets, config()))
}

/// A short synthetic meeting about a made-up product, in Italian.
fn short_transcript() -> Vec<NewSegment> {
    [
        (
            Role::Me,
            0,
            "Buongiorno, oggi decidiamo la data di lancio di Orsa.",
        ),
        (
            Role::Others,
            6_000,
            "Il collaudo finisce a fine aprile, quindi propongo maggio.",
        ),
        (Role::Me, 65_000, "D'accordo, lanciamo il 12 maggio."),
        (
            Role::Others,
            72_000,
            "Preparo io il piano di comunicazione entro venerdì.",
        ),
        (
            Role::Me,
            130_000,
            "Resta aperto il prezzo: ne parliamo la prossima settimana.",
        ),
    ]
    .into_iter()
    .map(|(role, start_ms, text)| NewSegment {
        role,
        start_ms,
        end_ms: start_ms + 5_000,
        text: text.into(),
    })
    .collect()
}

/// A long synthetic meeting: `count` distinct sentences, five seconds apart.
fn long_transcript(count: u32) -> Vec<NewSegment> {
    (0..count)
        .map(|i| NewSegment {
            role: if i % 3 == 0 { Role::Me } else { Role::Others },
            start_ms: i64::from(i) * 5_000,
            end_ms: i64::from(i) * 5_000 + 4_500,
            text: format!(
                "Frase {i}: parliamo del capitolo {} del progetto Orsa e di cosa resta da fare \
                 prima della consegna, senza decidere ancora niente di definitivo.",
                i / 10
            ),
        })
        .collect()
}

/// A meeting waiting for its summary, with `segments` as its transcription.
fn meeting(library: &mut Library, segments: &[NewSegment]) -> (Id, Version) {
    let meeting = library.create_meeting("it", ManualClock::START).unwrap();
    library
        .set_meeting_status(meeting.id, MeetingStatus::Processing, Some(600_000))
        .unwrap();
    let version = library
        .add_transcription(meeting.id, "parakeet-test", segments)
        .unwrap();
    (meeting.id, version)
}

/// A chat completion whose content is `content`.
fn chat(content: &str) -> Reply {
    Reply::json(
        200,
        json!({
            "model": MODEL,
            "choices": [{"message": {"content": content}, "finish_reason": "stop"}]
        })
        .to_string(),
    )
}

fn summary_reply(title: &str) -> Reply {
    chat(
        &json!({
            "title": title,
            "summary": "Il team fissa il lancio di Orsa al 12 maggio; il prezzo resta da decidere.",
            "key_points": [
                {"text": "Lancio fissato al 12 maggio.", "time": "00:01:04"},
                {"text": "Piano di comunicazione entro venerdì.", "time": "[00:01:12]"},
                {"text": "Prezzo ancora da decidere.", "time": "subito"}
            ]
        })
        .to_string(),
    )
}

/// The schema a request asks for (`meeting_summary` or `meeting_notes`).
fn schema_name(request: &Recorded) -> String {
    request.json()["response_format"]["json_schema"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn message(request: &Recorded, index: usize) -> String {
    request.json()["messages"][index]["content"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The first `[HH:MM:SS]` time in a text.
fn first_time(text: &str) -> String {
    let start = text.find('[').expect("a time in the input");
    text[start + 1..start + 9].to_owned()
}

/// Answers part and combine requests with notes of `notes_chars` characters, and the
/// final request with a summary.
fn responder(notes_chars: usize) -> impl Fn(&Recorded) -> Reply + Send + 'static {
    move |request| {
        let user = message(request, 1);
        if schema_name(request) == "meeting_notes" {
            let summary: String = "Si discute il progetto Orsa. "
                .chars()
                .cycle()
                .take(notes_chars)
                .collect();
            chat(
                &json!({
                    "summary": summary,
                    "key_points": [{"text": "Avanzamento del capitolo.", "time": first_time(&user)}]
                })
                .to_string(),
            )
        } else {
            chat(
                &json!({
                    "title": "Revisione del progetto Orsa",
                    "summary": "Si ripercorrono i capitoli del progetto Orsa.",
                    "key_points": [{"text": "Si parte dal primo capitolo.", "time": first_time(&user)}]
                })
                .to_string(),
            )
        }
    }
}

/// Checks that a request fits the model: estimated prompt plus reply within the context.
fn assert_fits(request: &Recorded, context: u32) {
    let body = request.json();
    let prompt: usize = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap().chars().count().div_ceil(3) + 8)
        .sum();
    let reply = body["max_tokens"].as_u64().unwrap() as usize;
    assert!(
        prompt + reply <= context as usize,
        "prompt {prompt} + reply {reply} > {context}"
    );
}

// --- Summaries ---

#[test]
fn a_meeting_gets_a_title_a_summary_and_key_points() {
    let server = MockServer::start();
    server.push(summary_reply("\"Data di lancio di Orsa\""));
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, Some(4_000));
    let (id, version) = meeting(&mut library, &short_transcript());

    let summary = summarize.summarize_now(&mut library, id).unwrap();
    assert_eq!(summary.version_id, version.id);
    assert_eq!(summary.title, "Data di lancio di Orsa");
    assert!(summary.body.starts_with("Il team fissa il lancio"));
    assert_eq!(
        summary.key_points,
        [
            // Times snap to the start of the nearest segment; an unreadable one is dropped.
            KeyPoint::at("Lancio fissato al 12 maggio.", 65_000),
            KeyPoint::at("Piano di comunicazione entro venerdì.", 72_000),
            KeyPoint::new("Prezzo ancora da decidere."),
        ]
    );
    assert_eq!(summary.language, "it");
    assert_eq!(summary.model, MODEL);
    assert_eq!(summary.prompt_version, PROMPT_VERSION);
    assert_eq!(library.latest_summary(id).unwrap(), Some(summary));

    let source = library.source(id).unwrap();
    assert_eq!(source.title, "Data di lancio di Orsa");
    assert_eq!(source.title_origin, TitleOrigin::Auto);
    assert_eq!(library.meeting(id).unwrap().status, MeetingStatus::Ready);

    // One request with the whole transcript, in the meeting language.
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let sent = requests[0].json();
    assert_eq!(sent["model"], MODEL);
    assert_eq!(sent["max_tokens"], summary::SUMMARY_OUTPUT_TOKENS);
    assert_eq!(schema_name(&requests[0]), "meeting_summary");
    assert!(message(&requests[0], 0).contains("Write in Italian (BCP 47 tag \"it\")"));
    let user = message(&requests[0], 1);
    assert!(user.contains("[00:01:05] Me: D'accordo, lanciamo il 12 maggio.\n"));
    for segment in short_transcript() {
        assert!(user.contains(&segment.text), "{}", segment.text);
    }
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

#[test]
fn the_summary_language_comes_from_settings_when_the_meeting_has_none() {
    let server = MockServer::start();
    server.push(summary_reply("Launch date"));
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);
    library.set_setting(MEETING_LANGUAGE_SETTING, "fr").unwrap();
    let (id, _) = meeting(&mut library, &short_transcript());
    // A meeting from before languages were recorded.
    let conn = rusqlite::Connection::open(library.root().join(DATABASE_FILE)).unwrap();
    conn.execute("UPDATE source SET language = NULL", [])
        .unwrap();

    let summary = summarize.summarize_now(&mut library, id).unwrap();
    assert_eq!(summary.language, "fr");
    assert!(message(&server.requests()[0], 0).contains("Write in French (BCP 47 tag \"fr\")"));
}

#[test]
fn long_transcripts_are_split_and_merged_without_losing_text() {
    const CONTEXT: u32 = 4_096;
    let server = MockServer::start_with(responder(200));
    let (_dir, _clock, mut library, summarize) = configured(&server, CONTEXT, None);
    let segments = long_transcript(150);
    let (id, _) = meeting(&mut library, &segments);

    let summary = summarize.summarize_now(&mut library, id).unwrap();
    assert_eq!(summary.title, "Revisione del progetto Orsa");
    assert_eq!(
        summary.key_points,
        [KeyPoint::at("Si parte dal primo capitolo.", 0)]
    );

    let requests = server.requests();
    let (last, parts) = requests.split_last().unwrap();
    assert!(parts.len() >= 3, "{} parts", parts.len());
    assert!(parts.iter().all(|r| schema_name(r) == "meeting_notes"
        && message(r, 0).starts_with("You take notes on one part")));
    assert_eq!(schema_name(last), "meeting_summary");
    assert!(message(last, 0).starts_with("You write the summary of a long meeting"));
    for request in &requests {
        assert_fits(request, CONTEXT);
    }

    // Every sentence is sent exactly once, whole, in order.
    let sent: String = parts.iter().map(|r| message(r, 1)).collect();
    let mut from = 0;
    for segment in &segments {
        let at = sent[from..]
            .find(&segment.text)
            .unwrap_or_else(|| panic!("missing: {}", segment.text));
        from += at + segment.text.len();
        assert_eq!(sent.matches(&segment.text).count(), 1, "{}", segment.text);
    }

    // The merge sees the notes on every part, with their times.
    let merge = message(last, 1);
    assert_eq!(merge.matches("Summary: ").count(), parts.len());
    assert!(merge.contains("Part 00:00:00 to "));
    assert!(merge.contains("- [00:00:00] Avanzamento del capitolo.\n"));
}

#[test]
fn notes_too_long_for_one_request_are_combined_first() {
    const CONTEXT: u32 = 4_096;
    let server = MockServer::start_with(responder(900));
    let (_dir, _clock, mut library, summarize) = configured(&server, CONTEXT, None);
    let (id, _) = meeting(&mut library, &long_transcript(700));

    summarize.summarize_now(&mut library, id).unwrap();

    let requests = server.requests();
    let systems: Vec<String> = requests.iter().map(|r| message(r, 0)).collect();
    let parts = systems
        .iter()
        .filter(|s| s.starts_with("You take notes on one part"))
        .count();
    let combines = systems
        .iter()
        .filter(|s| s.starts_with("You combine notes"))
        .count();
    assert!(parts > 10, "{parts} parts");
    assert!(combines >= 1 && combines < parts, "{combines} combines");
    assert!(systems.last().unwrap().starts_with("You write the summary"));
    for request in &requests {
        assert_fits(request, CONTEXT);
    }
}

#[test]
fn a_model_too_small_for_summaries_is_refused() {
    let server = MockServer::start();
    let (_dir, _clock, mut library, summarize) = configured(&server, 1_500, None);
    let (id, _) = meeting(&mut library, &short_transcript());

    let err = summarize.summarize_now(&mut library, id).unwrap_err();
    assert_eq!(err.code, "context_exceeded");
    assert!(!err.retryable);
    assert!(server.requests().is_empty());
}

#[test]
fn malformed_replies_are_asked_again_then_reported() {
    let server = MockServer::start();
    server
        .push(chat("Sure! Here is the summary of the meeting."))
        .push(chat(r#"{"title": "Lancio"}"#))
        .push(chat(r#"{"title": "", "summary": "...", "key_points": []}"#));
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);
    let (id, _) = meeting(&mut library, &short_transcript());

    let err = summarize.summarize_now(&mut library, id).unwrap_err();
    assert_eq!(err.code, "degenerate_output");
    assert!(err.retryable);
    assert_eq!(server.requests().len(), 3);
    assert_eq!(library.latest_summary(id).unwrap(), None);
    assert_eq!(library.source(id).unwrap().title, "");
    assert_eq!(
        library.meeting(id).unwrap().status,
        MeetingStatus::Processing
    );

    // A good reply after a bad one is enough.
    server
        .push(chat("```json\n{\"title\": 1}\n```"))
        .push(summary_reply("Lancio di Orsa"));
    let summary = summarize.summarize_now(&mut library, id).unwrap();
    assert_eq!(summary.title, "Lancio di Orsa");
}

#[test]
fn sources_without_words_are_not_sent() {
    let server = MockServer::start();
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);

    let note = library.create_note("Nota", "testo").unwrap();
    let err = summarize.summarize_now(&mut library, note.id).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.retryable),
        ("unsupported_source", false)
    );

    let waiting = library.create_meeting("it", ManualClock::START).unwrap();
    let err = summarize
        .summarize_now(&mut library, waiting.id)
        .unwrap_err();
    assert_eq!((err.code.as_str(), err.retryable), ("no_transcript", false));

    let silent = NewSegment {
        role: Role::Me,
        start_ms: 0,
        end_ms: 1_000,
        text: "   ".into(),
    };
    let (id, _) = meeting(&mut library, &[silent]);
    let err = summarize.summarize_now(&mut library, id).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.retryable),
        ("empty_transcript", false)
    );
    assert!(server.requests().is_empty());
}

#[test]
fn provider_failures_keep_their_codes() {
    let server = MockServer::start();
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);
    let (id, _) = meeting(&mut library, &short_transcript());

    server.push(Reply::json(401, r#"{"error":"bad key"}"#));
    let err = summarize.summarize_now(&mut library, id).unwrap_err();
    assert_eq!((err.code.as_str(), err.retryable), ("auth_failed", false));
    assert!(!err.detail.contains(KEY));

    library.clear_task_model(ModelTask::Summary).unwrap();
    let err = summarize.summarize_now(&mut library, id).unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.retryable),
        ("no_model_for_task", false)
    );
}

// --- As a job ---

/// Events from a runner, and a way to wait for one.
struct Events(Receiver<JobEvent>);

impl Events {
    fn sink() -> (Arc<dyn EventSink>, Self) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let sink = move |event: JobEvent| {
            let _ = tx.lock().unwrap().send(event);
        };
        (Arc::new(sink), Self(rx))
    }

    /// Waits for an event matching `want`, returning every event seen up to it.
    #[track_caller]
    fn wait_for(&self, want: impl Fn(&JobEvent) -> bool) -> Vec<JobEvent> {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(event) = self.0.recv_timeout(left) else {
                panic!("event not received; seen: {seen:#?}");
            };
            let done = want(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}

fn summary_settles(event: &JobEvent) -> bool {
    event.job.kind == JobKind::Summarize
        && event.progress.is_none()
        && matches!(
            event.job.state,
            JobState::Done | JobState::Failed | JobState::Queued
        )
        && event.job.attempts > 0
}

/// Stands in for the transcription engine (#26): stores a transcript, then queues the
/// summary through the hook.
struct FakeTranscription;

impl JobHandler for FakeTranscription {
    fn kind(&self) -> JobKind {
        JobKind::Transcribe
    }

    fn run(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        ctx.library()
            .add_transcription(job.target_id, "fake-engine", &short_transcript())?;
        summary::enqueue_after_transcription(ctx, job.target_id)?;
        Ok(())
    }
}

#[test]
fn a_transcription_queues_the_summary() {
    let server = MockServer::start();
    server.push(summary_reply("Data di lancio di Orsa"));
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);
    let id = library.create_meeting("it", ManualClock::START).unwrap().id;
    library
        .set_meeting_status(id, MeetingStatus::Processing, None)
        .unwrap();
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(FakeTranscription), Arc::new(summarize)],
        sink,
    );

    runner.enqueue(JobKind::Transcribe, id).unwrap();
    let seen = events.wait_for(summary_settles);
    let last = seen.last().unwrap();
    assert_eq!(last.job.state, JobState::Done, "{seen:#?}");
    assert!(seen.iter().any(|e| e.job.kind == JobKind::Summarize
        && e.job.state == JobState::Queued
        && e.job.attempts == 0));
    runner.stop();

    let library = lock(&library);
    assert_eq!(library.source(id).unwrap().title, "Data di lancio di Orsa");
    assert_eq!(library.meeting(id).unwrap().status, MeetingStatus::Ready);
}

#[test]
fn regenerating_keeps_a_title_written_by_the_user() {
    let titles = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&titles);
    let server = MockServer::start_with(move |_| {
        let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
        summary_reply(&format!("Titolo generato {n}"))
    });
    let (_dir, _clock, mut library, summarize) = configured(&server, 100_000, None);
    let (id, _) = meeting(&mut library, &short_transcript());
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(Arc::clone(&library), vec![Arc::new(summarize)], sink);

    summary::regenerate(&runner, id).unwrap();
    assert_eq!(
        events.wait_for(summary_settles).last().unwrap().job.state,
        JobState::Done
    );
    assert_eq!(
        lock(&library).source(id).unwrap().title,
        "Titolo generato 1"
    );

    lock(&library)
        .set_title(id, "Lancio di maggio", TitleOrigin::User)
        .unwrap();
    summary::regenerate(&runner, id).unwrap();
    assert_eq!(
        events.wait_for(summary_settles).last().unwrap().job.state,
        JobState::Done
    );
    runner.stop();

    let mut library = lock(&library);
    let source = library.source(id).unwrap();
    assert_eq!(
        (source.title.as_str(), source.title_origin),
        ("Lancio di maggio", TitleOrigin::User)
    );
    let latest = library.latest_summary(id).unwrap().unwrap();
    assert_eq!(latest.title, "Titolo generato 2");
    // Giving the title back takes the newest summary.
    assert_eq!(library.reset_title(id).unwrap().title, "Titolo generato 2");
}

#[test]
fn failures_end_in_the_job_with_a_code() {
    let server = MockServer::start_with(|_| Reply::json(503, r#"{"error":"overloaded"}"#));
    let (_dir, clock, mut library, summarize) = configured(&server, 100_000, None);
    let (id, _) = meeting(&mut library, &short_transcript());
    let waiting = library.create_meeting("it", ManualClock::START).unwrap().id;
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(Arc::clone(&library), vec![Arc::new(summarize)], sink);

    // A permanent error fails the job at once, with its code.
    let job = summary::regenerate(&runner, waiting).unwrap();
    let failed = events.wait_for(summary_settles).pop().unwrap().job;
    assert_eq!(failed.id, job.id);
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.error_code.as_deref(), Some("no_transcript"));
    assert_eq!(failed.attempts, 1);

    // A provider that keeps failing: the job waits and tries again later.
    let job = summary::regenerate(&runner, id).unwrap();
    let queued = events.wait_for(summary_settles).pop().unwrap().job;
    assert_eq!(queued.id, job.id);
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(queued.error_code.as_deref(), Some("server_error"));
    assert_eq!(queued.run_after, clock.now() + retry_delay(1));
    runner.stop();
    assert_eq!(lock(&library).latest_summary(id).unwrap(), None);
}
