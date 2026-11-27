"""Deterministic matcher between predicted and ground-truth entities of one source.

Matching is done per entity type, one-to-one, greedily by decreasing score.

People
    ``person_match_score`` (text.py): exact name/alias = 1.0, partial name that only one
    person of the source fits = 0.8, prediction more specific than the truth = 0.7.
    A pair matches when the score is > 0.

Decisions, actions, requirements, open questions
    score = max(s_text, 0.5 * s_kw + 0.5 * s_ev) where
      s_text = Dice of content tokens between predicted text and the true text
               (or any of its aliases), best of all;
      s_kw   = share of the truth's keywords whose tokens all appear in the predicted
               text or evidence;
      s_ev   = overlap coefficient between the predicted evidence quote and the true
               evidence quote (0 when either has fewer than 3 content tokens).
    A pair matches ("lenient match") when score >= TEXT_THRESHOLD (0.45).

Strict match (used for the headline precision/recall)
    a lenient match whose key fields are also right:
      actions: owner (``owner_match``) and status (open/closed);
      other types: same as lenient.
    A lenient match that fails the key fields counts as one false positive and one
    false negative in the strict figures, and as a field error.

Optional truth items (``"optional": true``) are borderline items: a prediction that
matches one is ignored (neither TP nor FP); an unmatched optional item is not a FN.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .text import (all_tokens, content_tokens, dice, overlap, owner_match,
                   person_match_score)

TEXT_THRESHOLD = 0.45
MIN_EVIDENCE_TOKENS = 3


@dataclass
class TypeResult:
    tp: int = 0                 # strict true positives
    fp: int = 0                 # strict false positives
    fn: int = 0                 # strict false negatives
    tp_lenient: int = 0
    fp_lenient: int = 0
    fn_lenient: int = 0
    absorbed: int = 0           # predictions matched to optional truth items
    fields: dict = field(default_factory=dict)  # name -> [correct, total]
    # (confidence, correct) for every non-absorbed prediction (strict correctness)
    conf: list = field(default_factory=list)
    pairs: list = field(default_factory=list)   # (pred_idx, truth_idx, score, strict_ok)
    unmatched_pred: list = field(default_factory=list)
    unmatched_truth: list = field(default_factory=list)

    def add_field(self, name: str, ok: bool) -> None:
        c = self.fields.setdefault(name, [0, 0])
        c[0] += int(ok)
        c[1] += 1


def text_score(pred: dict, truth: dict) -> float:
    ptext = set(content_tokens(pred.get("text")))
    pev = set(content_tokens(pred.get("evidence")))
    s_text = 0.0
    for cand in [truth.get("text", "")] + list(truth.get("aliases", [])):
        s_text = max(s_text, dice(ptext, set(content_tokens(cand))))
    kws = truth.get("keywords") or []
    s_kw = 0.0
    if kws:
        pool = ptext | pev
        hit = sum(1 for kw in kws if set(content_tokens(kw)) and set(content_tokens(kw)) <= pool)
        s_kw = hit / len(kws)
    tev = set(content_tokens(truth.get("evidence")))
    s_ev = overlap(pev, tev) if len(pev) >= MIN_EVIDENCE_TOKENS and len(tev) >= MIN_EVIDENCE_TOKENS else 0.0
    return max(s_text, 0.5 * s_kw + 0.5 * s_ev)


def _role_ok(pred_role: str | None, keywords: list[str]) -> bool:
    toks = set(all_tokens(pred_role))
    return any(set(all_tokens(k)) <= toks for k in keywords if all_tokens(k))


def _confidence(pred: dict) -> float | None:
    c = pred.get("confidence")
    if isinstance(c, bool) or not isinstance(c, (int, float)):
        return None
    return min(1.0, max(0.0, float(c)))


def match_type(typ: str, preds: list[dict], truths: list[dict], roster: list[str],
               extra_pairs: list[tuple[int, int]] | None = None) -> TypeResult:
    """Match predictions of one type against the truth items of one source.

    ``extra_pairs`` lets an external judge add (pred_idx, truth_idx) lenient matches
    for items the deterministic matcher left unmatched.
    """
    res = TypeResult()
    cands = []
    for i, p in enumerate(preds):
        for j, t in enumerate(truths):
            if typ == "people":
                s = person_match_score(p.get("name"), t["name"], t.get("aliases", []), roster)
                ok = s > 0
            else:
                s = text_score(p, t)
                ok = s >= TEXT_THRESHOLD
            if ok:
                cands.append((s, i, j))
    cands.sort(key=lambda x: (-x[0], x[1], x[2]))
    used_p, used_t, pairs = set(), set(), []
    for s, i, j in cands:
        if i in used_p or j in used_t:
            continue
        used_p.add(i)
        used_t.add(j)
        pairs.append((i, j, s))
    for i, j in extra_pairs or []:
        if i not in used_p and j not in used_t and 0 <= i < len(preds) and 0 <= j < len(truths):
            used_p.add(i)
            used_t.add(j)
            pairs.append((i, j, -1.0))  # -1 marks a judge match

    for i, j, s in pairs:
        p, t = preds[i], truths[j]
        if t.get("optional"):
            res.absorbed += 1
            continue
        strict_ok = True
        if typ == "people":
            if t.get("role") and t.get("role_keywords"):
                res.add_field("role", _role_ok(p.get("role"), t["role_keywords"]))
        elif typ == "decisions":
            if t.get("date"):
                res.add_field("date", str(p.get("date") or "")[:10] == t["date"])
            if t.get("reason"):
                res.add_field("reason_present", bool(p.get("reason")))
            res.add_field("supersedes", bool(p.get("supersedes")) == bool(t.get("supersedes")))
        elif typ == "actions":
            o_ok = owner_match(p.get("owner"), t.get("owner"), roster)
            s_ok = (p.get("status") or "").lower() == t["status"]
            res.add_field("owner", o_ok)
            res.add_field("status", s_ok)
            strict_ok = o_ok and s_ok
        res.tp_lenient += 1
        if strict_ok:
            res.tp += 1
        else:
            res.fp += 1
            res.fn += 1
        res.pairs.append((i, j, s, strict_ok))
        res.conf.append((_confidence(p), strict_ok))

    matched_p = {i for i, _, _ in pairs}
    matched_t = {j for _, j, _ in pairs}
    for i, p in enumerate(preds):
        if i not in matched_p:
            res.fp += 1
            res.fp_lenient += 1
            res.conf.append((_confidence(p), False))
            res.unmatched_pred.append(i)
    for j, t in enumerate(truths):
        if j not in matched_t and not t.get("optional"):
            res.fn += 1
            res.fn_lenient += 1
            res.unmatched_truth.append(j)
    return res


def prf(tp: int, fp: int, fn: int) -> tuple[float | None, float | None, float | None]:
    """Precision, recall, F1. None when undefined (no predictions / no truth)."""
    p = tp / (tp + fp) if tp + fp else None
    r = tp / (tp + fn) if tp + fn else None
    if p is None or r is None:
        f = None
    else:
        f = 2 * p * r / (p + r) if p + r else 0.0
    return p, r, f
