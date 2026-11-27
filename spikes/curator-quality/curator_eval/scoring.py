"""Scoring of recorded calls against the ground truth, and the Markdown summary.

Assignment (per source)
    predicted set = project ids returned (unknown ids dropped);
    exact   = predicted set equals the truth set (or one of ``acceptable_projects``);
    jaccard = |P∩T| / |P∪T| (1.0 when both are empty), the partial credit;
    a failed call counts as wrong with confidence 0 (it would go to review).
    Confidence of the decision = ``decision_confidence``.

Extraction (per source and type): see matching.py. Precision/recall/F1 are micro
averages over all sources of one repeat, then averaged over repeats (min–max shown).

Confidence usefulness: for each threshold t, among decisions with a confidence,
    errors caught   = share of wrong decisions with confidence < t (they go to review);
    correct to review = share of right decisions with confidence < t (wasted review);
    review load     = share of all decisions with confidence < t.
Missed items (false negatives) carry no confidence and can never be caught: they are
reported apart.
"""

from __future__ import annotations

import statistics
from collections import defaultdict

from .dataset import CORE_TYPES, ENTITY_TYPES, roster
from .matching import match_type, prf

THRESHOLDS = (0.5, 0.6, 0.7, 0.8, 0.9)


def _pct(x):
    return "–" if x is None else f"{100 * x:.0f}%"


def _f2(x):
    return "–" if x is None else f"{x:.2f}"


def percentile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    v = sorted(values)
    k = (len(v) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(v) - 1)
    return v[lo] + (v[hi] - v[lo]) * (k - lo)


def score_assignment(pred: dict | None, truth: dict) -> dict:
    tset = set(truth["projects"])
    acceptable = [set(a) for a in truth.get("acceptable_projects", [])] or [tset]
    if pred is None:
        return {"exact": False, "jaccard": 0.0, "pred": None, "conf": 0.0, "tp": 0, "fp": 0,
                "fn": len(tset), "items": [], "failed": True}
    pset = {a["project_id"] for a in pred["assignments"]}
    exact = any(pset == a for a in acceptable)
    jac = max((1.0 if not (pset | a) else len(pset & a) / len(pset | a)) for a in acceptable)
    conf = pred.get("decision_confidence")
    items = [(a.get("confidence"), a["project_id"] in tset) for a in pred["assignments"]]
    return {"exact": exact, "jaccard": jac, "pred": sorted(pset), "conf": conf,
            "tp": len(pset & tset), "fp": len(pset - tset), "fn": len(tset - pset),
            "items": items, "failed": False}


def confidence_table(pairs: list[tuple]) -> list[dict]:
    """pairs: (confidence or None, correct). Items without confidence are skipped."""
    pts = [(c, ok) for c, ok in pairs if c is not None]
    n_err = sum(1 for _, ok in pts if not ok)
    n_ok = len(pts) - n_err
    rows = []
    for t in THRESHOLDS:
        eb = sum(1 for c, ok in pts if not ok and c < t)
        cb = sum(1 for c, ok in pts if ok and c < t)
        rows.append({"threshold": t,
                     "errors_caught": eb / n_err if n_err else None,
                     "correct_to_review": cb / n_ok if n_ok else None,
                     "review_load": (eb + cb) / len(pts) if pts else None})
    return rows


def score_run(calls: list[dict], sources: list[dict], judge_pairs: dict | None = None) -> dict:
    """judge_pairs: {(model, source_id, repeat, type): [(pred_idx, truth_idx), ...]}"""
    judge_pairs = judge_pairs or {}
    by_src = {s["id"]: s for s in sources}
    latest = {}
    for c in calls:  # last record wins (resume / re-runs)
        if c["source_id"] in by_src:
            latest[(c["model"], c["source_id"], c["repeat"], c["task"])] = c
    models = sorted({k[0] for k in latest})
    out = {}
    for model in models:
        repeats = sorted({k[2] for k in latest if k[0] == model})
        per_rep = []
        conf_assign, conf_assign_items, conf_extract = [], [], []
        conf_extract_type = defaultdict(list)
        per_source_lat, per_source_cost = [], []
        assign_sets = defaultdict(list)
        failures = defaultdict(int)
        calls_n = defaultdict(int)
        tokens = defaultdict(int)
        reasoning = 0
        fields_total = defaultdict(lambda: [0, 0])
        ambiguous = []
        formats = defaultdict(int)
        per_source_detail = defaultdict(lambda: {"exact": [], "f1": []})
        for rep in repeats:
            a_exact, a_jac = [], []
            a_tp = a_fp = a_fn = 0
            counts = {t: defaultdict(int) for t in ENTITY_TYPES}
            for s in sources:
                sid, truth = s["id"], s["truth"]
                lat = cost = 0.0
                have_any = False
                for task in ("assignment", "extraction"):
                    c = latest.get((model, sid, rep, task))
                    if c is None:
                        continue
                    have_any = True
                    calls_n[task] += 1
                    if not c.get("ok"):
                        failures[task] += 1
                    if c.get("format"):
                        formats[c["format"]] += 1
                    lat += c.get("latency_s") or 0.0
                    cost += c.get("cost_usd") or 0.0
                    tokens["in"] += c.get("tokens_in") or 0
                    tokens["out"] += c.get("tokens_out") or 0
                    reasoning += c.get("reasoning_tokens") or 0
                if have_any:
                    per_source_lat.append(lat)
                    per_source_cost.append(cost)
                ca = latest.get((model, sid, rep, "assignment"))
                if ca is not None:
                    r = score_assignment(ca.get("parsed") if ca.get("ok") else None, truth)
                    a_exact.append(r["exact"])
                    a_jac.append(r["jaccard"])
                    a_tp += r["tp"]
                    a_fp += r["fp"]
                    a_fn += r["fn"]
                    conf_assign.append((r["conf"], r["exact"]))
                    conf_assign_items.extend(r["items"])
                    assign_sets[sid].append(tuple(r["pred"]) if r["pred"] is not None else None)
                    per_source_detail[sid]["exact"].append(r["exact"])
                    if truth.get("ambiguous"):
                        ambiguous.append({"source": sid, "pred": r["pred"], "conf": r["conf"]})
                ce = latest.get((model, sid, rep, "extraction"))
                if ce is not None:
                    parsed = ce.get("parsed") if ce.get("ok") else None
                    names = roster(truth)
                    stp = sfp = sfn = 0
                    for typ in ENTITY_TYPES:
                        preds = (parsed or {}).get(typ, [])
                        res = match_type(typ, preds, truth["entities"][typ], names,
                                         judge_pairs.get((model, sid, rep, typ)))
                        cn = counts[typ]
                        for k in ("tp", "fp", "fn", "tp_lenient", "fp_lenient", "fn_lenient", "absorbed"):
                            cn[k] += getattr(res, k)
                        for f, (ok, tot) in res.fields.items():
                            fields_total[f"{typ}.{f}"][0] += ok
                            fields_total[f"{typ}.{f}"][1] += tot
                        conf_extract_type[typ].extend(res.conf)
                        if typ in CORE_TYPES:
                            conf_extract.extend(res.conf)
                            stp, sfp, sfn = stp + res.tp, sfp + res.fp, sfn + res.fn
                    per_source_detail[sid]["f1"].append(prf(stp, sfp, sfn)[2])
            rep_res = {
                "assign_exact": sum(a_exact) / len(a_exact) if a_exact else None,
                "assign_jaccard": sum(a_jac) / len(a_jac) if a_jac else None,
                "assign_prf": prf(a_tp, a_fp, a_fn),
                "types": {},
            }
            core = defaultdict(int)
            for typ in ENTITY_TYPES:
                cn = counts[typ]
                rep_res["types"][typ] = {
                    "strict": prf(cn["tp"], cn["fp"], cn["fn"]),
                    "lenient": prf(cn["tp_lenient"], cn["fp_lenient"], cn["fn_lenient"]),
                    "counts": dict(cn),
                }
                if typ in CORE_TYPES:
                    for k in ("tp", "fp", "fn"):
                        core[k] += cn[k]
            rep_res["core_micro"] = prf(core["tp"], core["fp"], core["fn"])
            per_rep.append(rep_res)

        stable = [len(set(v)) == 1 for v in assign_sets.values() if len(v) > 1]
        out[model] = {
            "repeats": len(repeats),
            "per_repeat": per_rep,
            "agg": _aggregate(per_rep),
            "assign_stability": sum(stable) / len(stable) if stable else None,
            "conf_assignment": confidence_table(conf_assign),
            "conf_assignment_items": confidence_table(conf_assign_items),
            "conf_extraction": confidence_table(conf_extract),
            "conf_extraction_by_type": {t: confidence_table(v) for t, v in conf_extract_type.items()},
            "missing_conf": {
                "assignment": sum(1 for c, _ in conf_assign if c is None),
                "extraction": sum(1 for c, _ in conf_extract if c is None),
            },
            "latency_p50": percentile(per_source_lat, 0.5),
            "latency_p95": percentile(per_source_lat, 0.95),
            "cost_per_source": statistics.mean(per_source_cost) if per_source_cost else None,
            "cost_total": sum(per_source_cost),
            "tokens": dict(tokens),
            "reasoning_tokens": reasoning,
            "calls": dict(calls_n),
            "failures": dict(failures),
            "formats": dict(formats),
            "fields": {k: (v[0] / v[1] if v[1] else None, v[1]) for k, v in fields_total.items()},
            "ambiguous": ambiguous,
            "per_source": {sid: {"exact": sum(d["exact"]) / len(d["exact"]) if d["exact"] else None,
                                 "core_f1": (statistics.mean([f for f in d["f1"] if f is not None])
                                             if any(f is not None for f in d["f1"]) else None)}
                           for sid, d in per_source_detail.items()},
        }
    return out


def _mean_range(vals):
    vals = [v for v in vals if v is not None]
    if not vals:
        return (None, None, None)
    return (statistics.mean(vals), min(vals), max(vals))


def _aggregate(per_rep: list[dict]) -> dict:
    agg = {
        "assign_exact": _mean_range([r["assign_exact"] for r in per_rep]),
        "assign_jaccard": _mean_range([r["assign_jaccard"] for r in per_rep]),
        "assign_precision": _mean_range([r["assign_prf"][0] for r in per_rep]),
        "assign_recall": _mean_range([r["assign_prf"][1] for r in per_rep]),
        "core_f1": _mean_range([r["core_micro"][2] for r in per_rep]),
        "types": {},
    }
    for typ in ENTITY_TYPES:
        agg["types"][typ] = {
            "p": _mean_range([r["types"][typ]["strict"][0] for r in per_rep]),
            "r": _mean_range([r["types"][typ]["strict"][1] for r in per_rep]),
            "f1": _mean_range([r["types"][typ]["strict"][2] for r in per_rep]),
            "f1_lenient": _mean_range([r["types"][typ]["lenient"][2] for r in per_rep]),
        }
    return agg


# --------------------------------------------------------------------------- markdown

def _mr(t, pct=True):
    m, lo, hi = t
    if m is None:
        return "–"
    f = _pct if pct else _f2
    if lo is None or hi is None or abs(hi - lo) < 1e-9:
        return f(m)
    return f"{f(m)} ({f(lo)}–{f(hi)})"


def markdown_summary(scores: dict, title: str = "Curator quality — results", note: str = "") -> str:
    L = [f"## {title}", ""]
    if note:
        L += [note, ""]
    models = list(scores)
    L += ["### Overview (mean over repeats, min–max in brackets)", "",
          "| Model | Repeats | Assignment exact | Assignment partial (Jaccard) | Core entity F1 (strict) | Stability | Failed calls | Cost / source | Latency p50 / p95 per source |",
          "|---|---|---|---|---|---|---|---|---|"]
    for m in models:
        s = scores[m]
        a = s["agg"]
        fails = sum(s["failures"].values())
        calls = sum(s["calls"].values())
        cps = s["cost_per_source"]
        L.append(f"| {m} | {s['repeats']} | {_mr(a['assign_exact'])} | {_mr(a['assign_jaccard'], False)} | "
                 f"{_mr(a['core_f1'], False)} | {_pct(s['assign_stability'])} | {fails}/{calls} | "
                 f"{'–' if cps is None else f'${cps:.5f}'} | "
                 f"{_f2(s['latency_p50'])} s / {_f2(s['latency_p95'])} s |")
    L += ["", "### Entities: precision / recall / F1 per type (strict match; F1 lenient in the last column)", ""]
    L += ["| Model | Type | Precision | Recall | F1 | F1 lenient |", "|---|---|---|---|---|---|"]
    for m in models:
        for typ in ENTITY_TYPES:
            t = scores[m]["agg"]["types"][typ]
            label = typ + (" (optional type)" if typ not in CORE_TYPES else "")
            L.append(f"| {m} | {label} | {_mr(t['p'])} | {_mr(t['r'])} | {_mr(t['f1'], False)} | {_mr(t['f1_lenient'], False)} |")
    L += ["", "### Field accuracy on matched items", "",
          "| Model | " + " | ".join(["people.role", "decisions.date", "decisions.reason_present",
                                     "decisions.supersedes", "actions.owner", "actions.status"]) + " |",
          "|---|---|---|---|---|---|---|"]
    for m in models:
        f = scores[m]["fields"]
        cells = []
        for k in ["people.role", "decisions.date", "decisions.reason_present", "decisions.supersedes",
                  "actions.owner", "actions.status"]:
            v = f.get(k)
            cells.append("–" if not v or v[0] is None else f"{_pct(v[0])} (n={v[1]})")
        L.append(f"| {m} | " + " | ".join(cells) + " |")
    L += ["", "### Is confidence useful? Share of errors vs. correct answers below the review threshold", "",
          "Each cell: errors caught / correct answers sent to review / total review load.", "",
          "| Model | Task | " + " | ".join(f"t={t}" for t in THRESHOLDS) + " |",
          "|---|---|" + "---|" * len(THRESHOLDS)]
    for m in models:
        for label, key in (("assignment (source)", "conf_assignment"), ("entities (core types)", "conf_extraction")):
            rows = scores[m][key]
            cells = [f"{_pct(r['errors_caught'])} / {_pct(r['correct_to_review'])} / {_pct(r['review_load'])}" for r in rows]
            L.append(f"| {m} | {label} | " + " | ".join(cells) + " |")
    L += ["", "### Ambiguous source (S11): predictions and confidence", ""]
    for m in models:
        amb = scores[m]["ambiguous"]
        if amb:
            parts = [f"{a['pred'] or '[]'} @ {_f2(a['conf'])}" for a in amb]
            L.append(f"- {m}: " + "; ".join(parts))
    L += ["", "### Per source: assignment exact rate / core F1 (mean over repeats)", "",
          "| Source | " + " | ".join(models) + " |", "|---|" + "---|" * len(models)]
    sids = sorted({sid for m in models for sid in scores[m]["per_source"]})
    for sid in sids:
        cells = []
        for m in models:
            d = scores[m]["per_source"].get(sid)
            cells.append("–" if not d else f"{_pct(d['exact'])} / {_f2(d['core_f1'])}")
        L.append(f"| {sid} | " + " | ".join(cells) + " |")
    L += ["", "### Tokens and cost", "",
          "| Model | Calls | Tokens in | Tokens out | of which reasoning | Total cost | Formats used |",
          "|---|---|---|---|---|---|---|"]
    for m in models:
        s = scores[m]
        L.append(f"| {m} | {sum(s['calls'].values())} | {s['tokens'].get('in', 0)} | {s['tokens'].get('out', 0)} | "
                 f"{s['reasoning_tokens']} | ${s['cost_total']:.4f} | "
                 + ", ".join(f"{k}×{v}" for k, v in s["formats"].items()) + " |")
    total = sum(scores[m]["cost_total"] for m in models)
    L += ["", f"Total cost of the scored calls: ${total:.4f}", ""]
    return "\n".join(L)
