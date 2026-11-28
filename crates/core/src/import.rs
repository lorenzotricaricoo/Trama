//! Importing Markdown files as notes.
//!
//! Each `.md` file becomes a note: the title is the file name without the extension, the
//! text is the file's content as it is (only a UTF-8 byte order mark is left out), and the
//! note happened when the file was last modified. Files that are not Markdown, not UTF-8
//! text, larger than [`MAX_FILE_BYTES`] or already imported are skipped, each with a
//! [`SkipReason`] the interface can explain.
//!
//! A folder imports the Markdown files inside it, subfolders included; other files in it
//! are left alone without a message, since a folder of notes often holds images too.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::jobs::{SharedLibrary, lock};
use crate::storage::{Id, Library, Source, StorageError, Timestamp};

/// Largest file imported, in bytes. A note this long is already far beyond what anyone
/// writes by hand; bigger files are likely exports or data, not notes.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Most files imported at once; the rest are skipped with [`SkipReason::TooMany`].
pub const MAX_FILES: usize = 1000;

/// Extensions of the files imported as notes, compared ignoring case.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Longest title kept, in characters.
const MAX_TITLE_CHARS: usize = 200;

/// Files read and stored together: memory stays bounded however many files arrive.
const BATCH: usize = 32;

/// How deep a dropped folder is searched for Markdown files.
const MAX_DEPTH: usize = 16;

const UTF8_BOM: &str = "\u{feff}";

/// Why a file was not imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Not a `.md` or `.markdown` file (or not a file at all).
    NotMarkdown,
    /// Not UTF-8 text.
    NotText,
    /// Larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// The file could not be read (missing, no permission).
    Unreadable,
    /// A note with the same title and text already exists.
    Duplicate,
    /// More than [`MAX_FILES`] files at once.
    TooMany,
}

impl SkipReason {
    /// The code the interface translates.
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::NotMarkdown => "not_markdown",
            SkipReason::NotText => "not_text",
            SkipReason::TooLarge => "too_large",
            SkipReason::Unreadable => "unreadable",
            SkipReason::Duplicate => "duplicate",
            SkipReason::TooMany => "too_many",
        }
    }
}

/// A Markdown file read and ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownFile {
    pub title: String,
    pub body: String,
    /// When the file was last modified; the note's `occurred_at`.
    pub modified_at: Timestamp,
}

/// A file that was imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub path: PathBuf,
    pub id: Id,
    pub title: String,
}

/// A file that was skipped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// What an import did, in the order the files were given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: Vec<Imported>,
    pub skipped: Vec<Skipped>,
}

/// Whether the path has a Markdown extension.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            MARKDOWN_EXTENSIONS
                .iter()
                .any(|md| ext.eq_ignore_ascii_case(md))
        })
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

/// The files to import from what was dropped or chosen: files as they are, folders
/// replaced by the Markdown files inside them (sorted by path, hidden ones left out).
/// Paths given twice count once. Given files that are not Markdown are skipped here.
pub fn collect_files(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<Skipped>) {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found = Vec::new();
            walk(path, 0, &mut found);
            found.sort();
            files.extend(found);
        } else if is_markdown(path) {
            files.push(path.clone());
        } else {
            skipped.push(Skipped {
                path: path.clone(),
                reason: SkipReason::NotMarkdown,
            });
        }
    }
    let mut seen = std::collections::HashSet::new();
    files.retain(|path| seen.insert(path.clone()));
    if files.len() > MAX_FILES {
        skipped.extend(files.split_off(MAX_FILES).into_iter().map(|path| Skipped {
            path,
            reason: SkipReason::TooMany,
        }));
    }
    (files, skipped)
}

/// Adds the Markdown files under `dir` to `found`. Symbolic links are left out: a link
/// named `.md` inside a dropped folder could point anywhere, and a link to a folder could
/// trap the walk in a loop. Files the user picks or drops themselves are read as given.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_hidden(&path) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(&path, depth + 1, found);
        } else if is_markdown(&path) {
            found.push(path);
        }
    }
}

/// The title of a note imported from `path`: the file name without its extension, on one
/// line, at most [`MAX_TITLE_CHARS`] long.
pub fn title_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_default();
    let line = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(MAX_TITLE_CHARS).collect()
}

/// Reads a Markdown file to import.
pub fn read_markdown(path: &Path) -> Result<MarkdownFile, SkipReason> {
    if !is_markdown(path) {
        return Err(SkipReason::NotMarkdown);
    }
    let file = fs::File::open(path).map_err(|_| SkipReason::Unreadable)?;
    let metadata = file.metadata().map_err(|_| SkipReason::Unreadable)?;
    if !metadata.is_file() {
        return Err(SkipReason::NotMarkdown);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(SkipReason::TooLarge);
    }
    // The size may change while reading: never read more than the limit allows.
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SkipReason::Unreadable)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(SkipReason::TooLarge);
    }
    let text = String::from_utf8(bytes).map_err(|_| SkipReason::NotText)?;
    // Valid UTF-8 can still be binary data: text never holds NUL.
    if text.contains('\0') {
        return Err(SkipReason::NotText);
    }
    let body = text.strip_prefix(UTF8_BOM).unwrap_or(&text).to_owned();
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| Timestamp::try_from(since.as_millis()).ok());
    Ok(MarkdownFile {
        title: title_of(path),
        body,
        modified_at: modified_at.unwrap_or_else(now),
    })
}

fn now() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| Timestamp::try_from(since.as_millis()).ok())
        .unwrap_or_default()
}

/// Imports the files and folders in `paths` as notes into the shared library.
///
/// Files are read without holding the library, a batch at a time, and each batch is
/// stored while holding it, so other work on the library goes on during a long import.
/// Fails only if the library itself fails; problems with single files are reported in
/// [`ImportReport::skipped`].
pub fn import_files(
    paths: &[PathBuf],
    library: &SharedLibrary,
) -> Result<ImportReport, StorageError> {
    import_with(paths, |files| {
        let mut library = lock(library);
        files
            .iter()
            .map(|file| library.import_note(&file.title, &file.body, file.modified_at))
            .collect()
    })
}

/// Like [`import_files`], storing the files into an open library.
pub fn import_into(library: &mut Library, paths: &[PathBuf]) -> Result<ImportReport, StorageError> {
    import_with(paths, |files| {
        files
            .iter()
            .map(|file| library.import_note(&file.title, &file.body, file.modified_at))
            .collect()
    })
}

fn import_with(
    paths: &[PathBuf],
    mut store: impl FnMut(&[MarkdownFile]) -> Result<Vec<Option<Source>>, StorageError>,
) -> Result<ImportReport, StorageError> {
    let (files, mut skipped) = collect_files(paths);
    let mut report = ImportReport::default();
    for chunk in files.chunks(BATCH) {
        let mut read = Vec::with_capacity(chunk.len());
        let mut read_paths = Vec::with_capacity(chunk.len());
        for path in chunk {
            match read_markdown(path) {
                Ok(file) => {
                    read.push(file);
                    read_paths.push(path.clone());
                }
                Err(reason) => skipped.push(Skipped {
                    path: path.clone(),
                    reason,
                }),
            }
        }
        for (path, stored) in read_paths.into_iter().zip(store(&read)?) {
            match stored {
                Some(source) => report.imported.push(Imported {
                    path,
                    id: source.id,
                    title: source.title,
                }),
                None => skipped.push(Skipped {
                    path,
                    reason: SkipReason::Duplicate,
                }),
            }
        }
    }
    report.skipped = skipped;
    Ok(report)
}
