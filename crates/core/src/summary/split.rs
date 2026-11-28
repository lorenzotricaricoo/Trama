//! Splitting a transcript that doesn't fit the model into parts that do.
//!
//! Sizes are counted in tokens with a [`Sizer`], more pessimistic than the provider
//! client's estimate (three characters per token): prose of languages written with
//! ideographs or syllabaries counts one character per token, and the dense parts of each
//! line (the `[HH:MM:SS] Speaker: ` prefix, the headers of notes) two. Nothing is dropped:
//! every segment ends up whole in one part, or in consecutive parts if it is longer than a
//! part on its own.

use std::ops::Range;

use super::prompt::{Notes, render_note, segment_prefix};
use crate::storage::Segment;

/// Characters per token of the dense text around the words: times, speakers, headers.
const MARKUP_CHARS_PER_TOKEN: usize = 2;

/// How text is counted against a token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizer {
    /// Characters per token of the words, never more than the client's three.
    pub chars_per_token: usize,
}

impl Sizer {
    /// The sizer for prose in `language` (BCP 47): one character per token for Chinese,
    /// Japanese and Korean, where tokenizers often spend a token or more per character;
    /// three otherwise.
    pub fn for_language(language: &str) -> Self {
        let primary = language
            .split(['-', '_'])
            .next()
            .unwrap_or(language)
            .to_lowercase();
        let chars_per_token = match primary.as_str() {
            "ja" | "zh" | "ko" | "yue" => 1,
            _ => 3,
        };
        Self { chars_per_token }
    }

    /// Tokens of words.
    pub fn words(self, text: &str) -> usize {
        text.chars()
            .count()
            .div_ceil(self.chars_per_token.clamp(1, 3))
    }

    /// Tokens of markup (times, speakers, headers).
    pub fn markup(self, text: &str) -> usize {
        text.chars().count().div_ceil(MARKUP_CHARS_PER_TOKEN)
    }

    /// Tokens of a note as [`render_note`] writes it: its texts as words, the rest as
    /// markup.
    pub fn note(self, note: &Notes) -> usize {
        let rendered = render_note(note).chars().count();
        let summary = note.summary.trim();
        let mut words = self.words(summary);
        let mut word_chars = summary.chars().count();
        for (text, _) in &note.key_points {
            let text = text.trim();
            words += self.words(text);
            word_chars += text.chars().count();
        }
        words
            + rendered
                .saturating_sub(word_chars)
                .div_ceil(MARKUP_CHARS_PER_TOKEN)
    }
}

/// A run of consecutive transcript lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub start_ms: i64,
    pub end_ms: i64,
    /// The lines, each ending with a newline.
    pub text: String,
}

/// Splits the transcript into parts of at most `max_tokens` tokens, in order.
pub fn parts(segments: &[Segment], max_tokens: usize, sizer: Sizer) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut current: Option<(Part, usize)> = None;
    for segment in segments.iter().filter(|s| !s.text.trim().is_empty()) {
        for (line, tokens) in segment_lines(segment, max_tokens, sizer) {
            if let Some((_, used)) = &current
                && used + tokens > max_tokens
            {
                parts.extend(current.take().map(|(part, _)| part));
            }
            let (part, used) = current.get_or_insert_with(|| {
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
            *used += tokens;
        }
    }
    parts.extend(current.map(|(part, _)| part));
    parts
}

/// The lines of one segment with their tokens, each line ending with a newline: one line,
/// or several if the segment alone is longer than `max_tokens` (split between words, or
/// inside a word longer than a line).
fn segment_lines(segment: &Segment, max_tokens: usize, sizer: Sizer) -> Vec<(String, usize)> {
    let prefix = segment_prefix(segment);
    let prefix_tokens = sizer.markup(&prefix);
    let text = segment
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // The newline counts with the words.
    let line_tokens = |words: &str| prefix_tokens + sizer.words(words) + sizer.words("\n");
    if line_tokens(&text) <= max_tokens {
        return vec![(format!("{prefix}{text}\n"), line_tokens(&text))];
    }
    // Characters of words that fit on a line, with the newline.
    let room = max_tokens
        .saturating_sub(prefix_tokens + 1)
        .saturating_mul(sizer.chars_per_token.clamp(1, 3))
        .max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0;
    let mut flush = |line: &mut String, line_len: &mut usize| {
        if *line_len > 0 {
            lines.push((format!("{prefix}{line}\n"), line_tokens(line)));
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

/// Groups consecutive notes so the notes of each group fit in `max_tokens`.
pub fn group_notes(notes: &[Notes], max_tokens: usize, sizer: Sizer) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, note) in notes.iter().enumerate() {
        let tokens = sizer.note(note);
        if index > start && used + tokens > max_tokens {
            groups.push(start..index);
            start = index;
            used = 0;
        }
        used += tokens;
    }
    if start < notes.len() {
        groups.push(start..notes.len());
    }
    groups
}

/// Tokens of some notes, as [`group_notes`] counts them.
pub fn notes_tokens(notes: &[Notes], sizer: Sizer) -> usize {
    notes.iter().map(|note| sizer.note(note)).sum()
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
    use crate::providers::estimate_tokens;
    use crate::storage::{Id, Role};

    const LATIN: Sizer = Sizer { chars_per_token: 3 };

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
    fn languages_with_ideographs_count_a_token_per_character() {
        assert_eq!(Sizer::for_language("it"), LATIN);
        assert_eq!(Sizer::for_language("en-GB"), LATIN);
        for tag in ["ja", "zh-Hant", "ko_KR"] {
            assert_eq!(Sizer::for_language(tag).chars_per_token, 1, "{tag}");
        }
        assert_eq!(Sizer::for_language("ja").words("会議の要約"), 5);
        assert_eq!(LATIN.words("riunione"), 3);
    }

    #[test]
    fn sizes_never_undercount_the_client_estimate() {
        let segments: Vec<Segment> = (0..50)
            .map(|i| segment(i, i64::from(i) * 7_000, "Sì."))
            .collect();
        for sizer in [LATIN, Sizer::for_language("ja")] {
            for part in parts(&segments, 120, sizer) {
                assert!(estimate_tokens(&part.text) as usize <= 120);
            }
        }
    }

    #[test]
    fn the_line_prefix_counts_as_dense_text() {
        // "[00:00:00] Me: " is 15 characters: 8 tokens, plus 1 for "Sì" and 1 for the
        // newline.
        let segments: Vec<Segment> = (0..40).map(|i| segment(i * 2, 0, "Sì")).collect();
        let parts = parts(&segments, 90, LATIN);
        let lines: Vec<usize> = parts.iter().map(|p| p.text.lines().count()).collect();
        assert_eq!(lines, [9, 9, 9, 9, 4]);
        // Counting the prefix like words would have put 15 lines in a part.
        assert!(parts[0].text.chars().count().div_ceil(3) < 90 * 2 / 3);
    }

    #[test]
    fn a_short_transcript_is_one_part() {
        let segments = [segment(0, 0, "Ciao a tutti"), segment(1, 5_000, "Iniziamo")];
        let parts = parts(&segments, 10_000, LATIN);
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
        let parts = parts(&segments, 170, LATIN);
        assert!(parts.len() > 10);
        for part in &parts {
            assert!(part.text.chars().count() <= 510);
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
        let parts = parts(&segments, 100, LATIN);
        for part in &parts {
            assert!(estimate_tokens(&part.text) <= 100, "{}", part.text);
        }
        // A line holds (100 - 8 prefix tokens - 1 newline) * 3 characters of words.
        let mut expected = words(&segments[..2]);
        expected.extend(
            huge_word
                .chars()
                .collect::<Vec<_>>()
                .chunks((100 - 8 - 1) * 3)
                .map(|c| c.iter().collect::<String>()),
        );
        assert_eq!(part_words(&parts), expected);
    }

    #[test]
    fn blank_segments_are_skipped() {
        let segments = [segment(0, 0, "  "), segment(1, 1_000, "Sì")];
        assert_eq!(
            parts(&segments, 1_000, LATIN)[0].text,
            "[00:00:01] Others: Sì\n"
        );
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
            key_points: vec![("Deciso".into(), Some(i * 60_000))],
        };
        let notes: Vec<Notes> = (0..5).map(note).collect();
        let one = LATIN.note(&notes[0]);
        assert!(one >= estimate_tokens(&render_note(&notes[0])) as usize);
        assert_eq!(notes_tokens(&notes, LATIN), one * 5);
        assert_eq!(group_notes(&notes, one * 2, LATIN), [0..2, 2..4, 4..5]);
        assert_eq!(group_notes(&notes, one * 10, LATIN), vec![0..5]);
        // A note too long for a group still gets one of its own.
        assert_eq!(
            group_notes(&notes, 1, LATIN),
            [0..1, 1..2, 2..3, 3..4, 4..5]
        );
    }
}
