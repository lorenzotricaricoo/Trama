//! Text normalization and word error rate (WER).
//!
//! Normalization (applied identically to reference and hypothesis):
//! 1. Unicode NFKC, then lowercase.
//! 2. Every character that is not a letter or a digit (punctuation, apostrophes,
//!    hyphens, symbols) becomes a space. So "l'azienda" -> "l azienda",
//!    "follow-up" -> "follow up", "Ciao, Marco!" -> "ciao marco".
//! 3. Split on whitespace.
//!
//! Accents are kept ("è" != "e"). Numbers are NOT normalized ("3" != "tre"),
//! fillers are NOT removed. WER = (S + D + I) / N, with N = reference words.
//! The corpus-level WER sums S, D, I and N over all files (not an average of
//! per-file WERs).

use unicode_normalization::UnicodeNormalization;

pub fn normalize(text: &str) -> Vec<String> {
    let lowered: String = text.nfkc().collect::<String>().to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().map(str::to_owned).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Ok(String),
    Sub(String, String),
    Del(String),
    Ins(String),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub n: usize,
    pub sub: usize,
    pub del: usize,
    pub ins: usize,
}

impl Counts {
    pub fn errors(&self) -> usize {
        self.sub + self.del + self.ins
    }
    pub fn wer(&self) -> Option<f64> {
        (self.n > 0).then(|| 100.0 * self.errors() as f64 / self.n as f64)
    }
    pub fn add(&mut self, o: &Counts) {
        self.n += o.n;
        self.sub += o.sub;
        self.del += o.del;
        self.ins += o.ins;
    }
}

/// Levenshtein alignment over words; returns the edit operations.
pub fn align(r: &[String], h: &[String]) -> (Counts, Vec<Op>) {
    let (n, m) = (r.len(), h.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = d[i - 1][j - 1] + usize::from(r[i - 1] != h[j - 1]);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    let (mut i, mut j) = (n, m);
    let mut ops = Vec::new();
    let mut c = Counts { n, ..Default::default() };
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + usize::from(r[i - 1] != h[j - 1]) {
            if r[i - 1] == h[j - 1] {
                ops.push(Op::Ok(r[i - 1].clone()));
            } else {
                c.sub += 1;
                ops.push(Op::Sub(r[i - 1].clone(), h[j - 1].clone()));
            }
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            c.del += 1;
            ops.push(Op::Del(r[i - 1].clone()));
            i -= 1;
        } else {
            c.ins += 1;
            ops.push(Op::Ins(h[j - 1].clone()));
            j -= 1;
        }
    }
    ops.reverse();
    (c, ops)
}

/// Renders the alignment, marking errors as {ref→hyp}, {ref→∅}, {∅→hyp}.
pub fn render(ops: &[Op]) -> String {
    ops.iter()
        .map(|op| match op {
            Op::Ok(w) => w.clone(),
            Op::Sub(r, h) => format!("{{{r}→{h}}}"),
            Op::Del(r) => format!("{{{r}→∅}}"),
            Op::Ins(h) => format!("{{∅→{h}}}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_punctuation_and_apostrophes() {
        assert_eq!(normalize("L'azienda, OK? Follow-up è pronto."),
            vec!["l", "azienda", "ok", "follow", "up", "è", "pronto"]);
    }

    #[test]
    fn counts_edits() {
        let r = normalize("il deploy di oggi è pronto");
        let h = normalize("il deployment oggi è pronto davvero");
        let (c, _) = align(&r, &h);
        assert_eq!((c.n, c.sub, c.del, c.ins), (6, 1, 1, 1));
    }
}
