//! Repository tests: a real library on disk, driven headless through the core API.

mod common;

use std::path::PathBuf;

use common::{DAY, MINUTE, ManualClock, library};
use tempfile::TempDir;
use trama_core::storage::*;

fn segment(role: Role, start_ms: i64, text: &str) -> NewSegment {
    NewSegment {
        role,
        start_ms,
        end_ms: start_ms + 2_000,
        text: text.to_owned(),
    }
}

fn track(meeting: Id, role: Role) -> NewTrack {
    let dir = Library::meeting_audio_dir(meeting);
    NewTrack {
        role,
        audio_path: dir.join(format!("{}.wav", role.as_str())),
        timing_path: Some(dir.join(format!("{}.timing", role.as_str()))),
        device: Some("Test device".into()),
    }
}

fn summary(title: &str) -> NewSummary {
    NewSummary {
        title: title.into(),
        body: "Body".into(),
        key_points: vec!["One".into(), "Two \"quoted\"".into()],
        language: "it".into(),
        model: "test-model".into(),
        prompt_version: "1".into(),
    }
}

/// A meeting with two tracks and a transcription.
fn recorded_meeting(library: &mut Library, segments: &[NewSegment]) -> (Source, Version) {
    let meeting = library.create_meeting("it", ManualClock::START).unwrap();
    library
        .add_track(meeting.id, &track(meeting.id, Role::Me))
        .unwrap();
    library
        .add_track(meeting.id, &track(meeting.id, Role::Others))
        .unwrap();
    let version = library
        .add_transcription(meeting.id, "parakeet-ultra-fp32", segments)
        .unwrap();
    (meeting, version)
}

fn hit_sources(hits: &[SearchHit]) -> Vec<Id> {
    hits.iter().map(|hit| hit.source_id).collect()
}

#[test]
fn creates_the_library_layout() {
    let (dir, _, library) = library();
    assert!(dir.path().join(DATABASE_FILE).is_file());
    assert!(dir.path().join(LOCK_FILE).is_file());
    assert!(dir.path().join(AUDIO_DIR).is_dir());
    assert_eq!(library.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn data_survives_reopening() {
    let dir = TempDir::new().unwrap();
    let id = {
        let mut library = Library::open(dir.path()).unwrap();
        library.create_note("Kept", "Text").unwrap().id
    };
    let library = Library::open(dir.path()).unwrap();
    assert_eq!(library.source(id).unwrap().title, "Kept");
    assert_eq!(library.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn only_one_process_owns_the_library() {
    let (dir, _, library) = library();
    let err = Library::open(dir.path()).unwrap_err();
    assert!(matches!(err, StorageError::Locked(_)), "{err}");
    drop(library);
    Library::open(dir.path()).unwrap();
}

#[test]
fn refuses_a_library_from_a_newer_version() {
    let dir = TempDir::new().unwrap();
    drop(Library::open(dir.path()).unwrap());
    let conn = rusqlite_connection(&dir.path().join(DATABASE_FILE));
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(conn);
    let err = Library::open(dir.path()).unwrap_err();
    assert!(matches!(err, StorageError::NewerSchema { .. }), "{err}");
}

#[test]
fn records_a_meeting_with_tracks_and_a_transcription() {
    let (_dir, clock, mut library) = library();
    let (meeting, version) = recorded_meeting(
        &mut library,
        &[
            segment(Role::Me, 0, "Ciao a tutti"),
            segment(Role::Others, 2_500, "Buongiorno"),
        ],
    );
    assert_eq!(meeting.kind, SourceKind::Meeting);
    assert_eq!(meeting.language.as_deref(), Some("it"));
    assert_eq!(
        library.meeting(meeting.id).unwrap().status,
        MeetingStatus::Recording
    );

    clock.advance(MINUTE);
    let updated = library
        .set_meeting_status(meeting.id, MeetingStatus::Ready, Some(65_000))
        .unwrap();
    assert_eq!(updated.status, MeetingStatus::Ready);
    assert_eq!(updated.duration_ms, Some(65_000));

    let tracks = library.tracks(meeting.id).unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].role, Role::Me);
    assert!(tracks[0].audio_path.starts_with(AUDIO_DIR));

    assert_eq!(version.number, 1);
    assert!(version.is_sealed());
    assert_eq!(version.engine.as_deref(), Some("parakeet-ultra-fp32"));
    let segments = library.segments(version.id).unwrap();
    assert_eq!(
        segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        ["Ciao a tutti", "Buongiorno"]
    );
    assert_eq!(segments[1].role, Role::Others);
    assert_eq!(segments[1].seq, 1);
}

#[test]
fn one_track_per_role_and_paths_inside_the_library() {
    let (_dir, _, mut library) = library();
    let meeting = library.create_meeting("en", ManualClock::START).unwrap();
    library
        .add_track(meeting.id, &track(meeting.id, Role::Me))
        .unwrap();
    assert!(
        library
            .add_track(meeting.id, &track(meeting.id, Role::Me))
            .is_err()
    );

    let outside = NewTrack {
        audio_path: PathBuf::from("/Users/someone/me.wav"),
        ..track(meeting.id, Role::Others)
    };
    assert!(matches!(
        library.add_track(meeting.id, &outside),
        Err(StorageError::Invalid(_))
    ));
}

#[test]
fn a_new_transcription_becomes_the_current_version() {
    let (_dir, _, mut library) = library();
    let (meeting, first) = recorded_meeting(&mut library, &[segment(Role::Me, 0, "prima bozza")]);
    let second = library
        .add_transcription(
            meeting.id,
            "better-engine",
            &[segment(Role::Me, 0, "seconda versione")],
        )
        .unwrap();
    assert_eq!(second.number, 2);
    assert_eq!(
        library.current_version(meeting.id).unwrap().unwrap().id,
        second.id
    );
    assert_eq!(library.versions(meeting.id).unwrap().len(), 2);
    // The old version stays as it was, for what cites it.
    assert_eq!(library.segments(first.id).unwrap()[0].text, "prima bozza");

    assert!(library.search("bozza", 10).unwrap().is_empty());
    assert_eq!(
        hit_sources(&library.search("seconda", 10).unwrap()),
        [meeting.id]
    );
}

#[test]
fn completing_a_transcription_stores_it_and_marks_the_meeting_ready_together() {
    let (_dir, _, mut library) = library();
    let meeting = library.create_meeting("it", ManualClock::START).unwrap();
    library
        .set_meeting_failed(meeting.id, "model_missing")
        .unwrap();
    let version = library
        .complete_transcription(meeting.id, "engine", &[segment(Role::Me, 0, "fatto")])
        .unwrap();
    assert!(version.is_sealed());
    assert_eq!(library.segments(version.id).unwrap()[0].text, "fatto");
    let state = library.meeting(meeting.id).unwrap();
    assert_eq!(state.status, MeetingStatus::Ready);
    assert_eq!(state.error_code, None);

    // In the trash, nothing at all is written.
    let trashed = library.create_meeting("it", ManualClock::START).unwrap();
    library
        .set_meeting_status(trashed.id, MeetingStatus::Processing, None)
        .unwrap();
    library.trash(trashed.id).unwrap();
    assert!(matches!(
        library.complete_transcription(trashed.id, "engine", &[segment(Role::Me, 0, "no")]),
        Err(StorageError::NotFound { .. })
    ));
    assert!(library.versions(trashed.id).unwrap().is_empty());
    assert_eq!(
        library.meeting(trashed.id).unwrap().status,
        MeetingStatus::Processing
    );
}

#[test]
fn note_autosave_coalesces_edits_into_versions() {
    let (_dir, clock, mut library) = library();
    let note = library.create_note("", "Prima").unwrap();
    assert_eq!(note.title_origin, TitleOrigin::Auto);

    clock.advance(MINUTE);
    let v1 = library.save_note(note.id, "Prima riga").unwrap();
    clock.advance(9 * MINUTE);
    let v1_again = library.save_note(note.id, "Prima riga, ancora").unwrap();
    assert_eq!(v1.id, v1_again.id);
    assert_eq!(v1_again.number, 1);
    assert!(!v1_again.is_sealed());

    // Saving the same text changes nothing.
    clock.advance(30 * MINUTE);
    assert_eq!(
        library.save_note(note.id, "Prima riga, ancora").unwrap().id,
        v1.id
    );

    // After a pause, the next edit starts a new version and seals the old one.
    let v2 = library.save_note(note.id, "Dopo la pausa").unwrap();
    assert_eq!(v2.number, 2);
    let versions = library.versions(note.id).unwrap();
    assert_eq!(versions.len(), 2);
    assert!(versions[0].is_sealed());
    assert_eq!(versions[0].body.as_deref(), Some("Prima riga, ancora"));
    assert_eq!(versions[0].sealed_at, Some(versions[0].edited_at));

    // The curator seals the open version before reading it; edits then start a new one.
    let sealed = library.seal_current_version(note.id).unwrap().unwrap();
    assert_eq!(sealed.id, v2.id);
    assert!(sealed.is_sealed());
    clock.advance(MINUTE);
    assert_eq!(
        library
            .save_note(note.id, "Dopo la lettura")
            .unwrap()
            .number,
        3
    );
}

#[test]
fn sealed_versions_and_segments_never_change() {
    let (dir, _, mut library) = library();
    let (_, version) = recorded_meeting(&mut library, &[segment(Role::Me, 0, "fisso")]);
    let note = library.create_note("N", "testo").unwrap();
    let note_version = library.seal_current_version(note.id).unwrap().unwrap();
    drop(library);

    let conn = rusqlite_connection(&dir.path().join(DATABASE_FILE));
    let edit_note = conn.execute(
        "UPDATE source_version SET body = 'altro' WHERE id = ?1",
        [note_version.id.to_string()],
    );
    assert!(
        edit_note
            .unwrap_err()
            .to_string()
            .contains("sealed version")
    );
    let edit_segment = conn.execute("UPDATE transcript_segment SET text = 'altro'", []);
    assert!(edit_segment.is_err());
    let add_segment = conn.execute(
        "INSERT INTO transcript_segment (version_id, seq, role, start_ms, end_ms, text)
         VALUES (?1, 99, 'me', 0, 1, 'aggiunto')",
        [version.id.to_string()],
    );
    assert!(add_segment.is_err());
}

#[test]
fn summaries_title_the_source_unless_the_user_did() {
    let (_dir, clock, mut library) = library();
    let (meeting, version) = recorded_meeting(&mut library, &[segment(Role::Me, 0, "ok")]);

    let first = library
        .add_summary(version.id, &summary("Titolo automatico"))
        .unwrap();
    assert_eq!(first.key_points, ["One", "Two \"quoted\""]);
    assert_eq!(
        library.source(meeting.id).unwrap().title,
        "Titolo automatico"
    );

    library
        .set_title(meeting.id, "Mio titolo", TitleOrigin::User)
        .unwrap();
    clock.advance(MINUTE);
    let second = library
        .add_summary(version.id, &summary("Altro titolo"))
        .unwrap();
    assert_eq!(library.source(meeting.id).unwrap().title, "Mio titolo");
    library
        .set_title(meeting.id, "Ignorato", TitleOrigin::Auto)
        .unwrap();
    assert_eq!(library.source(meeting.id).unwrap().title, "Mio titolo");

    assert_eq!(
        library.latest_summary(meeting.id).unwrap().unwrap().id,
        second.id
    );
}

#[test]
fn recording_notes_stay_editable() {
    let (_dir, clock, mut library) = library();
    let (meeting, _) = recorded_meeting(&mut library, &[]);
    let late = library
        .add_recording_note(meeting.id, 90_000, "dopo")
        .unwrap();
    let early = library
        .add_recording_note(meeting.id, 5_000, "decisione sul budget")
        .unwrap();
    assert_eq!(
        library
            .recording_notes(meeting.id)
            .unwrap()
            .iter()
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        [early.id, late.id]
    );

    clock.advance(DAY);
    let edited = library
        .update_recording_note(early.id, "decisione sul calendario")
        .unwrap();
    assert_eq!(edited.updated_at, clock.now());
    assert!(library.search("budget", 10).unwrap().is_empty());
    let hits = library.search("calendario", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].target,
        HitTarget::RecordingNote { note_id: early.id }
    );

    library.delete_recording_note(early.id).unwrap();
    assert!(library.search("calendario", 10).unwrap().is_empty());
    assert!(matches!(
        library.delete_recording_note(early.id),
        Err(StorageError::NotFound { .. })
    ));
}

#[test]
fn search_ignores_case_and_accents_and_matches_prefixes() {
    let (_dir, _, mut library) = library();
    let (meeting, version) = recorded_meeting(
        &mut library,
        &[
            segment(Role::Me, 0, "Parliamo del rilascio"),
            segment(Role::Others, 3_000, "Perché la Città è in ritardo?"),
        ],
    );
    let note = library
        .create_note("", "L'università chiede il rilascio")
        .unwrap();

    let hits = library.search("perche citta", 10).unwrap();
    assert_eq!(hits.len(), 1);
    let segments = library.segments(version.id).unwrap();
    assert_eq!(
        hits[0].target,
        HitTarget::Segment {
            version_id: version.id,
            segment_id: segments[1].id
        }
    );
    assert!(
        hits[0]
            .excerpt
            .contains(&format!("{HIGHLIGHT_START}Perché{HIGHLIGHT_END}"))
    );

    let mut found = hit_sources(&library.search("RILASC", 10).unwrap());
    found.sort();
    let mut expected = vec![meeting.id, note.id];
    expected.sort();
    assert_eq!(found, expected);

    let note_hits = library.search("universita", 10).unwrap();
    let note_version = library.current_version(note.id).unwrap().unwrap();
    assert_eq!(
        note_hits[0].target,
        HitTarget::Note {
            version_id: note_version.id
        }
    );

    assert!(library.search("NEAR(\"", 10).unwrap().is_empty());
    assert!(library.search("   ", 10).unwrap().is_empty());
}

#[test]
fn search_follows_note_edits() {
    let (_dir, clock, mut library) = library();
    let note = library.create_note("", "vecchio testo").unwrap();
    clock.advance(MINUTE);
    library.save_note(note.id, "nuovo testo").unwrap();
    assert!(library.search("vecchio", 10).unwrap().is_empty());

    clock.advance(20 * MINUTE);
    library.save_note(note.id, "testo definitivo").unwrap();
    assert!(
        library.search("nuovo", 10).unwrap().is_empty(),
        "old versions are not searched"
    );
    assert_eq!(
        hit_sources(&library.search("definitivo", 10).unwrap()),
        [note.id]
    );
}

#[test]
fn lists_sources_newest_first_with_paging() {
    let (_dir, clock, mut library) = library();
    let mut ids = Vec::new();
    for title in ["a", "b", "c"] {
        clock.advance(MINUTE);
        ids.push(library.create_note(title, "").unwrap().id);
    }
    let meeting = library.create_meeting("it", clock.now() + MINUTE).unwrap();

    let all = library.list_sources(None, None, 10).unwrap();
    assert_eq!(
        all.iter().map(|s| s.id).collect::<Vec<_>>(),
        [meeting.id, ids[2], ids[1], ids[0]]
    );

    let notes = library
        .list_sources(Some(SourceKind::Note), None, 2)
        .unwrap();
    assert_eq!(
        notes.iter().map(|s| s.id).collect::<Vec<_>>(),
        [ids[2], ids[1]]
    );
    let older = library
        .list_sources(Some(SourceKind::Note), Some(notes[1].occurred_at), 2)
        .unwrap();
    assert_eq!(older.iter().map(|s| s.id).collect::<Vec<_>>(), [ids[0]]);
}

#[test]
fn trash_hides_sources_until_restored() {
    let (_dir, _, mut library) = library();
    let (meeting, _) = recorded_meeting(&mut library, &[segment(Role::Me, 0, "riservato")]);
    let note = library.create_note("Nota", "riservato anche qui").unwrap();

    library.trash(meeting.id).unwrap();
    assert_eq!(library.list_sources(None, None, 10).unwrap().len(), 1);
    assert_eq!(
        hit_sources(&library.search("riservato", 10).unwrap()),
        [note.id]
    );
    assert_eq!(library.list_trash().unwrap()[0].id, meeting.id);
    assert!(library.source(meeting.id).unwrap().deleted_at.is_some());
    assert!(matches!(
        library.add_recording_note(meeting.id, 0, "x"),
        Err(StorageError::NotFound { .. })
    ));

    library.restore(meeting.id).unwrap();
    assert_eq!(library.list_sources(None, None, 10).unwrap().len(), 2);
    assert_eq!(library.search("riservato", 10).unwrap().len(), 2);
    assert!(library.list_trash().unwrap().is_empty());
}

#[test]
fn purging_the_trash_removes_everything_and_returns_audio() {
    let (dir, clock, mut library) = library();
    let (meeting, version) =
        recorded_meeting(&mut library, &[segment(Role::Me, 0, "da eliminare")]);
    library
        .add_recording_note(meeting.id, 1_000, "nota da eliminare")
        .unwrap();
    library
        .add_summary(version.id, &summary("Riassunto"))
        .unwrap();
    let recent = library
        .create_note("Recente", "da eliminare più tardi")
        .unwrap();

    library.trash(meeting.id).unwrap();
    clock.advance(TRASH_RETENTION_DAYS * DAY);
    library.trash(recent.id).unwrap();

    let purged = library
        .purge_trash(clock.now() - TRASH_RETENTION_DAYS * DAY + 1)
        .unwrap();
    assert_eq!(purged.sources, [meeting.id]);
    let audio_dir = Library::meeting_audio_dir(meeting.id);
    assert_eq!(purged.audio_paths.len(), 5);
    assert!(purged.audio_paths.iter().all(|p| p.starts_with(&audio_dir)));
    assert!(purged.audio_paths.contains(&audio_dir));

    assert!(matches!(
        library.source(meeting.id),
        Err(StorageError::NotFound { .. })
    ));
    assert_eq!(library.list_trash().unwrap()[0].id, recent.id);
    drop(library);

    let conn = rusqlite_connection(&dir.path().join(DATABASE_FILE));
    let count = |sql: &str| -> i64 {
        conn.query_row(sql, [meeting.id.to_string()], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(
        count("SELECT COUNT(*) FROM source_version WHERE source_id = ?1"),
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM meeting WHERE source_id = ?1"),
        0
    );
    assert_eq!(count("SELECT COUNT(*) FROM track WHERE meeting_id = ?1"), 0);
    assert_eq!(
        count("SELECT COUNT(*) FROM recording_note WHERE meeting_id = ?1"),
        0
    );
    // Only the purged meeting had segments and summaries.
    assert_eq!(
        count("SELECT COUNT(*) FROM transcript_segment WHERE ?1 IS NOT NULL"),
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM summary WHERE ?1 IS NOT NULL"),
        0
    );
    let indexed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM segment_search WHERE segment_search MATCH 'eliminare'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 0);
    let indexed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM text_search WHERE source_id = ?1",
            [meeting.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 0);
}

#[test]
fn audio_deletion_is_recorded() {
    let (_dir, clock, mut library) = library();
    let (meeting, _) = recorded_meeting(&mut library, &[]);
    clock.advance(DAY);
    let updated = library.mark_audio_deleted(meeting.id).unwrap();
    assert_eq!(updated.audio_deleted_at, Some(clock.now()));
    clock.advance(DAY);
    assert_eq!(
        library
            .mark_audio_deleted(meeting.id)
            .unwrap()
            .audio_deleted_at,
        updated.audio_deleted_at
    );
}

#[test]
fn settings_round_trip() {
    let (_dir, _, mut library) = library();
    assert_eq!(library.setting("meeting.language").unwrap(), None);
    library.set_setting("meeting.language", "it").unwrap();
    library.set_setting("meeting.language", "en").unwrap();
    assert_eq!(
        library.setting("meeting.language").unwrap().as_deref(),
        Some("en")
    );
}

#[test]
fn backs_up_a_consistent_copy() {
    let (dir, _, mut library) = library();
    let note = library.create_note("Salvata", "nel backup").unwrap();
    let backup_dir = TempDir::new().unwrap();
    library
        .backup_database(backup_dir.path().join(DATABASE_FILE))
        .unwrap();
    drop(library);
    drop(dir);

    let restored = Library::open(backup_dir.path()).unwrap();
    assert_eq!(restored.source(note.id).unwrap().title, "Salvata");
    assert_eq!(
        hit_sources(&restored.search("backup", 10).unwrap()),
        [note.id]
    );
}

#[test]
fn unknown_ids_are_reported() {
    let (_dir, _, mut library) = library();
    let missing = Id::now_v7();
    assert!(matches!(
        library.source(missing),
        Err(StorageError::NotFound { .. })
    ));
    assert!(matches!(
        library.meeting(missing),
        Err(StorageError::NotFound { .. })
    ));
    assert!(matches!(
        library.save_note(missing, "x"),
        Err(StorageError::NotFound { .. })
    ));
    assert!(matches!(
        library.trash(missing),
        Err(StorageError::NotFound { .. })
    ));
    assert!(matches!(
        library.add_transcription(missing, "e", &[]),
        Err(StorageError::NotFound { .. })
    ));
    // A note is not a meeting.
    let note = library.create_note("N", "").unwrap();
    assert!(matches!(
        library.add_recording_note(note.id, 0, "x"),
        Err(StorageError::NotFound { .. })
    ));
}

/// A raw connection, for checks the API deliberately does not allow.
fn rusqlite_connection(path: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).unwrap()
}
