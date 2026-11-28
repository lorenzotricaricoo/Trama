mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use common::{MINUTE, library};
use tempfile::TempDir;
use trama_core::import::{
    MAX_FILE_BYTES, SkipReason, collect_files, import_files, import_into, read_markdown, title_of,
};
use trama_core::storage::*;

/// Writes a file modified at `modified_ms` (milliseconds since the epoch).
fn write(dir: &Path, name: &str, bytes: &[u8], modified_ms: u64) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, bytes).unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_millis(modified_ms))
        .unwrap();
    path
}

const MAY_2025: u64 = 1_746_000_000_000;

fn reasons(report: &trama_core::import::ImportReport) -> Vec<(String, SkipReason)> {
    report
        .skipped
        .iter()
        .map(|s| {
            (
                s.path.file_name().unwrap().to_string_lossy().into_owned(),
                s.reason,
            )
        })
        .collect()
}

#[test]
fn each_markdown_file_becomes_a_note_with_its_name_text_and_date() {
    let files = TempDir::new().unwrap();
    let text = "---\ntags: [q3]\n---\n# Piano Q3\r\n\r\n- budget *fermo*\n";
    let path = write(files.path(), "Piano Q3.md", text.as_bytes(), MAY_2025);
    let (_dir, clock, mut library) = library();

    let report = import_into(&mut library, &[path]).unwrap();
    assert!(report.skipped.is_empty());
    let note = library.source(report.imported[0].id).unwrap();
    assert_eq!(note.kind, SourceKind::Note);
    assert_eq!(note.title, "Piano Q3");
    assert_eq!(note.title_origin, TitleOrigin::User);
    assert_eq!(note.occurred_at, MAY_2025 as i64);
    assert_eq!(note.created_at, clock.now());
    // The text exactly as in the file: front matter and line endings included.
    let version = library.current_version(note.id).unwrap().unwrap();
    assert_eq!(version.body.as_deref(), Some(text));
    assert_eq!(version.origin, VersionOrigin::Editor);
    assert!(
        version.is_sealed(),
        "the imported text stays in the history"
    );

    // Editing the note starts a new version over the imported one.
    clock.advance(MINUTE);
    let edited = library.save_note(note.id, "# Piano Q3, rivisto").unwrap();
    assert_eq!(edited.number, 2);
    assert_eq!(
        library.versions(note.id).unwrap()[0].body.as_deref(),
        Some(text)
    );
}

#[test]
fn imported_notes_are_listed_by_date_and_found_by_search() {
    let files = TempDir::new().unwrap();
    let old = write(
        files.path(),
        "vecchia.md",
        b"retrospettiva di aprile",
        MAY_2025,
    );
    let new = write(
        files.path(),
        "nuova.markdown",
        b"Revisione del budget",
        MAY_2025 + 86_400_000,
    );
    let (_dir, _clock, mut library) = library();
    let report = import_into(&mut library, &[old, new]).unwrap();
    assert_eq!(report.imported.len(), 2);

    let listed: Vec<String> = library
        .list_sources(Some(SourceKind::Note), None, 10)
        .unwrap()
        .into_iter()
        .map(|source| source.title)
        .collect();
    assert_eq!(listed, ["nuova", "vecchia"]);
    let found = library.search_sources("retrospett", None, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].item.source.title, "vecchia");
    assert_eq!(library.source_counts().unwrap().notes, 2);
}

#[test]
fn files_that_are_not_markdown_text_or_too_big_are_skipped() {
    let files = TempDir::new().unwrap();
    let dir = files.path();
    let good = write(dir, "ok.md", "perché sì".as_bytes(), MAY_2025);
    let latin1 = write(dir, "latin1.md", b"perch\xe9", MAY_2025);
    let binary = write(dir, "nul.md", b"a\0b", MAY_2025);
    let image = write(dir, "foto.png", b"\x89PNG", MAY_2025);
    let big = write(
        dir,
        "big.md",
        &vec![b'x'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1],
        MAY_2025,
    );
    let at_limit = write(
        dir,
        "limit.md",
        &vec![b'y'; usize::try_from(MAX_FILE_BYTES).unwrap()],
        MAY_2025,
    );
    let missing = dir.join("gone.md");
    let (_dir, _clock, mut library) = library();

    let report = import_into(
        &mut library,
        &[good, latin1, binary, image, big, at_limit, missing],
    )
    .unwrap();
    let titles: Vec<&str> = report.imported.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["ok", "limit"]);
    let mut skipped = reasons(&report);
    skipped.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        skipped,
        [
            ("big.md".into(), SkipReason::TooLarge),
            ("foto.png".into(), SkipReason::NotMarkdown),
            ("gone.md".into(), SkipReason::Unreadable),
            ("latin1.md".into(), SkipReason::NotText),
            ("nul.md".into(), SkipReason::NotText),
        ]
    );
}

#[test]
fn the_same_file_imported_again_is_a_duplicate() {
    let files = TempDir::new().unwrap();
    let path = write(files.path(), "Diario.md", b"oggi", MAY_2025);
    let (_dir, _clock, mut library) = library();
    let first = import_into(&mut library, std::slice::from_ref(&path)).unwrap();
    assert_eq!(first.imported.len(), 1);

    let again = import_into(&mut library, std::slice::from_ref(&path)).unwrap();
    assert!(again.imported.is_empty());
    assert_eq!(
        reasons(&again),
        [("Diario.md".into(), SkipReason::Duplicate)]
    );

    // Once its text changed, or the note is in the trash, it imports again.
    fs::write(&path, b"oggi e domani").unwrap();
    assert_eq!(
        import_into(&mut library, std::slice::from_ref(&path))
            .unwrap()
            .imported
            .len(),
        1
    );
    library.trash(first.imported[0].id).unwrap();
    fs::write(&path, b"oggi").unwrap();
    assert_eq!(
        import_into(&mut library, &[path]).unwrap().imported.len(),
        1
    );
}

#[test]
fn a_folder_imports_the_markdown_files_inside_it() {
    let files = TempDir::new().unwrap();
    let root = files.path().join("Vault");
    write(&root, "b.md", b"b", MAY_2025);
    write(&root, "a.md", b"a", MAY_2025);
    write(&root, "sub/c.MD", b"c", MAY_2025);
    write(&root, "img.png", b"png", MAY_2025);
    write(&root, ".obsidian/workspace.md", b"hidden", MAY_2025);
    write(&root, ".hidden.md", b"hidden", MAY_2025);
    let a = root.join("a.md");
    // Links inside a dropped folder are not followed.
    #[cfg(unix)]
    {
        let secret = write(files.path(), "secret.txt", b"private", MAY_2025);
        std::os::unix::fs::symlink(&secret, root.join("link.md")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
    }

    // A file given on its own and inside a dropped folder counts once.
    let (found, skipped) = collect_files(&[root.clone(), a]);
    assert!(skipped.is_empty(), "other files in a folder are left alone");
    let names: Vec<_> = found
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["a.md", "b.md", "sub/c.MD"]);

    // The shared library is held only while storing.
    let dir = TempDir::new().unwrap();
    let shared = Arc::new(Mutex::new(Library::open(dir.path()).unwrap()));
    let report = import_files(&[root], &shared).unwrap();
    assert_eq!(report.imported.len(), 3);
}

#[test]
fn titles_come_from_the_file_name() {
    assert_eq!(title_of(Path::new("/x/Weekly  sync\t.md")), "Weekly sync");
    assert_eq!(title_of(Path::new("/x/note.v2.md")), "note.v2");
    assert_eq!(
        title_of(&PathBuf::from(format!("/x/{}.md", "è".repeat(300))))
            .chars()
            .count(),
        200
    );
}

#[test]
fn a_byte_order_mark_is_not_part_of_the_text() {
    let files = TempDir::new().unwrap();
    let path = write(
        files.path(),
        "bom.md",
        "\u{feff}# Titolo".as_bytes(),
        MAY_2025,
    );
    assert_eq!(read_markdown(&path).unwrap().body, "# Titolo");
}
