//! Splitting a transcript that doesn't fit the model into parts that do.
//!
//! Sizes are counted in characters: the token estimate of the provider client is one
//! token every three characters, rounded up, so `max_chars = budget * 3` keeps a part
//! within `budget` tokens. Nothing is dropped: every segment ends up whole in one part, or
//! in consecutive parts if it is longer than a part on its own.

use std::ops::Range;

use super::prompt::{Notes, render_note, segment_prefix};
use crate::storage::Segment;

/// A run of consecutive transcript lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub start_ms: i64,
    pub end_ms: i64,
    /// The lines, each ending with a newline.
    pub text: String,
}

/// Splits the transcript into parts of at most `max_chars` characters, in order.
pub fn parts(segments: &[Segment], max_chars: usize) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut current: Option<(Part, usize)> = None;
    for segment in segments.iter().filter(|s| !s.text.trim().is_empty()) {
        for line in segment_lines(segment, max_chars) {
            let len = line.chars().count();
            if let Some((_, chars)) = &current
                && chars + len > max_chars
            {
                parts.extend(current.take().map(|(part, _)| part));
            }
            let (part, chars) = current.get_or_insert_with(|| {
                (
                    Part {
                        start_ms: segment.start_ms,
                        end_ms: segment.end_ms,
                        text: String::new(),
                    },
                    0,
                )
            });
            part.text.push_str(&line);
            part.end_ms = part.end_ms.max(segment.end_ms);
            *chars += len;
        }
    }
    parts.extend(current.map(|(part, _)| part));
    parts
}

/// The lines of one segment, each ending with a newline: one line, or several if the
/// segment alone is longer than `max_chars` (split between words, or inside a word longer
/// than a line).
fn segment_lines(segment: &Segment, max_chars: usize) -> Vec<String> {
    let prefix = segment_prefix(segment);
    let text = segment
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let prefix_len = prefix.chars().count();
    if prefix_len + text.chars().count() < max_chars {
        return vec![format!("{prefix}{text}\n")];
    }
    // Room for text on a line, after the prefix and before the newline.
    let room = max_chars.saturating_sub(prefix_len + 1).max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0;
    let mut flush = |line: &mut String, line_len: &mut usize| {
        if *line_len > 0 {
            lines.push(format!("{prefix}{line}\n"));
            line.clear();
            *line_len = 0;
        }
    };
    for word in text.split(' ') {
        let mut word: Vec<char> = word.chars().collect();
        while !word.is_empty() {
            let gap = usize::from(line_len > 0);
            if line_len + gap + word.len() <= room {
                if gap == 1 {
                    line.push(' ');
                }
                line_len += gap + word.len();
                line.extend(word.drain(..));
            } else if line_len > 0 {
                flush(&mut line, &mut line_len);
            } else {
                // A word longer than a whole line: cut it.
                line.extend(word.drain(..room));
                line_len = room;
                flush(&mut line, &mut line_len);
            }
        }
    }
    flush(&mut line, &mut line_len);
    lines
}

/// Groups consecutive notes so the rendered notes of each group fit in `max_chars`.
pub fn group_notes(notes: &[Notes], max_chars: usize) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    for (index, note) in notes.iter().enumerate() {
        let len = render_note(note).chars().count();
        if index > start && chars + len > max_chars {
            groups.push(start..index);
            start = index;
            chars = 0;
        }
        chars += len;
    }
    if start < notes.len() {
        groups.push(start..notes.len());
    }
    groups
}

/// The start of the segment nearest to `at_ms`: a key point points to where the
/// transcript says it, never between segments or past the end.
pub fn snap(at_ms: i64, segments: &[Segment]) -> Option<i64> {
    segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .map(|s| s.start_ms)
        .min_by_key(|start| (start - at_ms).abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Id, Role};

    fn segment(seq: u32, start_ms: i64, text: &str) -> Segment {
        Segment {
            id: i64::from(seq),
            version_id: Id::nil(),
            seq,
            role: if seq.is_multiple_of(2) {
                Role::Me
            } else {
                Role::Others
            },
            start_ms,
            end_ms: start_ms + 4_000,
            text: text.into(),
        }
    }

    /// The words of every segment, in order.
    fn words(segments: &[Segment]) -> Vec<String> {
        segments
            .iter()
            .flat_map(|s| s.text.split_whitespace().map(str::to_owned))
            .collect()
    }

    /// The words of every part, without the line prefixes.
    fn part_words(parts: &[Part]) -> Vec<String> {
        parts
            .iter()
            .flat_map(|p| p.text.lines())
            .flat_map(|line| {
                let text = line.split_once(": ").expect("a prefixed line").1;
                text.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn a_short_transcript_is_one_part() {
        let segments = [segment(0, 0, "Ciao a tutti"), segment(1, 5_000, "Iniziamo")];
        let parts = parts(&segments, 10_000);
        assert_eq!(
            parts,
            [Part {
                start_ms: 0,
                end_ms: 9_000,
                text: "[00:00:00] Me: Ciao a tutti\n[00:00:05] Others: Iniziamo\n".into()
            }]
        );
    }

    #[test]
    fn long_transcripts_are_split_without_losing_text() {
        let segments: Vec<Segment> = (0..200)
            .map(|i| {
                segment(
                    i,
                    i64::from(i) * 5_000,
                    &format!("frase numero {i} del meeting"),
                )
            })
            .collect();
        let parts = parts(&segments, 500);
        assert!(parts.len() > 10);
        for part in &parts {
            assert!(part.text.chars().count() <= 500);
        }
        assert_eq!(part_words(&parts), words(&segments));
        // Parts follow each other in time.
        for pair in parts.windows(2) {
            assert!(pair[0].end_ms <= pair[1].start_ms);
        }
    }

    #[test]
    fn a_segment_longer_than_a_part_is_cut_between_words() {
        let long = (0..300)
            .map(|i| format!("parola{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let huge_word = "x".repeat(700);
        let segments = [
            segment(0, 0, "Prima"),
            segment(1, 60_000, &long),
            segment(2, 120_000, &huge_word),
        ];
        let parts = parts(&segments, 300);
        for part in &parts {
            assert!(part.text.chars().count() <= 300, "{}", part.text);
        }
        let mut expected = words(&segments[..2]);
        expected.extend(
            huge_word
                .chars()
                .collect::<Vec<_>>()
                .chunks(300 - "[00:02:00] Me: ".len() - 1)
                .map(|c| c.iter().collect::<String>()),
        );
        assert_eq!(part_words(&parts), expected);
    }

    #[test]
    fn blank_segments_are_skipped() {
        let segments = [segment(0, 0, "  "), segment(1, 1_000, "Sì")];
        assert_eq!(parts(&segments, 1_000)[0].text, "[00:00:01] Others: Sì\n");
        assert_eq!(snap(0, &segments), Some(1_000));
    }

    #[test]
    fn times_snap_to_the_nearest_segment() {
        let segments = [
            segment(0, 0, "a"),
            segment(1, 10_000, "b"),
            segment(2, 30_000, "c"),
        ];
        assert_eq!(snap(4_000, &segments), Some(0));
        assert_eq!(snap(16_000, &segments), Some(10_000));
        assert_eq!(snap(999_000, &segments), Some(30_000));
        assert_eq!(snap(0, &[]), None);
    }

    #[test]
    fn notes_are_grouped_to_fit() {
        let note = |i: i64| Notes {
            start_ms: i * 60_000,
            end_ms: (i + 1) * 60_000,
            summary: "x".repeat(50),
            key_points: vec![],
        };
        let notes: Vec<Notes> = (0..5).map(note).collect();
        let one = render_note(&notes[0]).chars().count();
        assert_eq!(group_notes(&notes, one * 2), [0..2, 2..4, 4..5]);
        assert_eq!(group_notes(&notes, one * 10), vec![0..5]);
        // A note too long for a group still gets one of its own.
        assert_eq!(group_notes(&notes, 1), [0..1, 1..2, 2..3, 3..4, 4..5]);
    }
}
