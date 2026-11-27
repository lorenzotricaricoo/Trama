"""Token and cost estimate of one full run (all sources, both tasks), no API calls.

Input tokens  = system + user messages (json_schema format) + the JSON schema itself
                (providers inject the schema into the context), via ``approx_tokens``.
Output tokens = the ideal answer built from the ground truth (each item with a 15-word
                evidence quote, a confidence, and for assignment a 20-word reason),
                times OUTPUT_SLACK (models write longer texts and extra items).
Reasoning     = optional multiplier on output for reasoning models (hidden tokens are
                billed as output).
"""

from __future__ import annotations

import json

from .prompts import PromptKit
from .text import approx_tokens

OUTPUT_SLACK = 1.5
EVIDENCE_WORDS = 15
REASON = "La fonte parla del cliente e dei temi del progetto, con riferimenti espliciti alle persone coinvolte."


def expected_output(task: str, truth: dict) -> str:
    if task == "assignment":
        return json.dumps({"assignments": [{"project_id": p, "confidence": 0.93, "reason": REASON}
                                           for p in truth["projects"]],
                           "decision_confidence": 0.9}, ensure_ascii=False)
    out = {}
    for typ, items in truth["entities"].items():
        res = []
        for e in items:
            ev = " ".join(e.get("evidence", "").split()[:EVIDENCE_WORDS])
            if typ == "people":
                d = {"name": e["name"], "role": e.get("role")}
            else:
                d = {"text": e["text"]}
                if typ == "decisions":
                    d.update(date=e.get("date"), reason=e.get("reason"), supersedes=e.get("supersedes"))
                if typ == "actions":
                    d.update(owner=e.get("owner"), status=e["status"])
            d.update(evidence=ev, confidence=0.87)
            res.append(d)
        out[typ] = res
    return json.dumps(out, ensure_ascii=False)


def token_profile(kit: PromptKit, sources: list[dict]) -> dict:
    prof = {"assignment": {"in": 0, "out": 0}, "extraction": {"in": 0, "out": 0}, "sources": len(sources),
            "source_text": 0, "per_source": []}
    for s in sources:
        row = {"id": s["id"], "text": approx_tokens(s["text"])}
        prof["source_text"] += row["text"]
        for task in ("assignment", "extraction"):
            msgs = kit.messages(task, s, "json_schema")
            tin = sum(approx_tokens(m["content"]) for m in msgs) + approx_tokens(json.dumps(kit.schema(task)))
            tout = int(approx_tokens(expected_output(task, s["truth"])) * OUTPUT_SLACK)
            prof[task]["in"] += tin
            prof[task]["out"] += tout
            row[task] = (tin, tout)
        prof["per_source"].append(row)
    return prof


def run_cost(prof: dict, price_in: float, price_out: float, reasoning_mult: float = 1.0) -> float:
    tin = prof["assignment"]["in"] + prof["extraction"]["in"]
    tout = (prof["assignment"]["out"] + prof["extraction"]["out"]) * reasoning_mult
    return (tin * price_in + tout * price_out) / 1e6
