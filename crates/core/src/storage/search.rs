//! Full-text search over transcripts, notes and notes typed while recording.

use rusqlite::params;

use super::{HitTarget, Library, Result, SearchHit, StorageError, parse_id};

/// Words of context around a match in an excerpt.
const EXCERPT_TOKENS: u32 = 16;

/// Turns what the user typed into an FTS5 query: every word must appear, as a prefix,
/// so results show up while typing. FTS5 operators typed by the user are treated as
/// plain text. Returns `None` when there is nothing to search for.
pub(super) fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{word}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

impl Library {
    /// Searches the current version of every source not in the trash. Case and accents
    /// are ignored. Results are ordered by relevance.
    pub fn search(&self, input: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let Some(query) = fts_query(input) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT source_id, kind, ref, version_id, excerpt, rank FROM (
                 SELECT v.source_id AS source_id, 'segment' AS kind, CAST(seg.id AS TEXT) AS ref,
                        seg.version_id AS version_id,
                        snippet(segment_search, 0, char(2), char(3), '…', {EXCERPT_TOKENS}) AS excerpt,
                        bm25(segment_search) AS rank
                 FROM segment_search
                 JOIN transcript_segment seg ON seg.id = segment_search.rowid
                 JOIN source_version v ON v.id = seg.version_id
                 JOIN source s ON s.id = v.source_id
                 WHERE segment_search MATCH ?1
                   AND s.deleted_at IS NULL
                   AND v.number = (SELECT MAX(number) FROM source_version WHERE source_id = v.source_id)
                 UNION ALL
                 SELECT text_search.source_id, text_search.kind, text_search.ref,
                        CASE text_search.kind WHEN 'note' THEN text_search.ref END,
                        snippet(text_search, 0, char(2), char(3), '…', {EXCERPT_TOKENS}),
                        bm25(text_search)
                 FROM text_search
                 JOIN source s ON s.id = text_search.source_id
                 WHERE text_search MATCH ?1
                   AND s.deleted_at IS NULL
                   AND (text_search.kind = 'recording_note'
                        OR text_search.ref = (SELECT id FROM source_version
                                              WHERE source_id = text_search.source_id
                                              ORDER BY number DESC LIMIT 1))
             )
             ORDER BY rank, source_id, ref
             LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![query, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, f64>(5)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (source_id, kind, reference, version_id, excerpt, rank) = row?;
            let version_id = version_id.as_deref().map(parse_id).transpose()?;
            let target = match (kind.as_str(), version_id) {
                ("segment", Some(version_id)) => HitTarget::Segment {
                    version_id,
                    segment_id: reference.parse().map_err(|_| {
                        StorageError::Corrupt(format!("invalid segment id {reference:?}"))
                    })?,
                },
                ("note", Some(version_id)) => HitTarget::Note { version_id },
                ("recording_note", None) => HitTarget::RecordingNote {
                    note_id: parse_id(&reference)?,
                },
                _ => {
                    return Err(StorageError::Corrupt(format!(
                        "invalid search entry {kind:?}"
                    )));
                }
            };
            hits.push(SearchHit {
                source_id: parse_id(&source_id)?,
                target,
                excerpt,
                rank,
            });
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn builds_prefix_queries() {
        assert_eq!(
            fts_query("budget Q3").as_deref(),
            Some("\"budget\"* \"Q3\"*")
        );
        assert_eq!(fts_query("  perché  ").as_deref(), Some("\"perché\"*"));
    }

    #[test]
    fn neutralizes_fts_syntax() {
        assert_eq!(
            fts_query("a\" OR b NEAR(").as_deref(),
            Some("\"a\"* \"OR\"* \"b\"* \"NEAR(\"*")
        );
        assert_eq!(fts_query("\" - *"), None);
        assert_eq!(fts_query(""), None);
    }
}
