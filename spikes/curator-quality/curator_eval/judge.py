"""Optional LLM judge (OFF by default, enabled with --judge-model).

After the deterministic matcher, for each (model, source, repeat, type) that still has
unmatched predictions AND unmatched truth items, one judge call is asked which of
those predictions denote the same item as which truth item (paraphrases the token
matcher missed). Judge matches are added as lenient matches; key fields (action owner
and status) are still checked deterministically. Judge calls count towards the budget
and are recorded in judge.jsonl.
"""

from __future__ import annotations

import json
import time

from .dataset import ENTITY_TYPES, roster
from .matching import match_type
from .prompts import parse_json_content
from .provider import BudgetExceeded, ProviderError
from .text import approx_tokens

SCHEMA = {
    "type": "object",
    "properties": {"pairs": {"type": "array", "items": {
        "type": "object",
        "properties": {"pred": {"type": "integer"}, "truth": {"type": "integer"}},
        "required": ["pred", "truth"], "additionalProperties": False}}},
    "required": ["pairs"], "additionalProperties": False,
}

SYSTEM = ("You compare items extracted from a meeting or note against expected items. Two items match "
          "when they denote the same {typ} (same content), even if worded differently or in another "
          "language. Ignore owner, status, dates. Each item can be used at most once. Return only "
          "confident matches as pairs of indices.")


def _desc(typ, e):
    return e.get("name") if typ == "people" else e.get("text")


def run_judge(client, judge_model, calls, sources, pricing, budget, out_path, log=print) -> dict:
    by_src = {s["id"]: s for s in sources}
    pairs = {}
    latest = {}
    for c in calls:
        if c["task"] == "extraction" and c.get("ok") and c["source_id"] in by_src:
            latest[(c["model"], c["source_id"], c["repeat"])] = c
    with open(out_path, "a", encoding="utf-8") as f:
        for (model, sid, rep), c in sorted(latest.items()):
            truth = by_src[sid]["truth"]
            for typ in ENTITY_TYPES:
                preds = c["parsed"].get(typ, [])
                res = match_type(typ, preds, truth["entities"][typ], roster(truth))
                if not res.unmatched_pred or not res.unmatched_truth:
                    continue
                up = {i: _desc(typ, preds[i]) for i in res.unmatched_pred}
                ut = {j: _desc(typ, truth["entities"][typ][j]) for j in res.unmatched_truth}
                user = json.dumps({"extracted": up, "expected": ut}, ensure_ascii=False)
                payload = {"model": judge_model, "temperature": 0,
                           "messages": [{"role": "system", "content": SYSTEM.format(typ=typ)},
                                        {"role": "user", "content": user}],
                           "max_tokens": 300,
                           "response_format": {"type": "json_schema", "json_schema": {
                               "name": "judge", "strict": True, "schema": SCHEMA}}}
                worst = pricing.cost(judge_model, approx_tokens(user) + 200, 300)
                try:
                    budget.reserve(worst)
                except BudgetExceeded as e:
                    log(f"judge STOP: {e}")
                    return pairs
                rec = {"judge_model": judge_model, "model": model, "source_id": sid, "repeat": rep,
                       "type": typ, "ts": time.strftime("%Y-%m-%dT%H:%M:%S")}
                try:
                    resp, lat = client.chat(payload, meta={"judge": True, "source_id": sid, "task": "judge"})
                except ProviderError as e:
                    budget.settle(worst, 0.0)
                    rec["error"] = str(e)
                    f.write(json.dumps(rec) + "\n")
                    continue
                u = resp.get("usage") or {}
                cost = pricing.cost(judge_model, u.get("prompt_tokens", 0), u.get("completion_tokens", 0))
                budget.settle(worst, cost)
                content = resp["choices"][0]["message"].get("content")
                obj, err = parse_json_content(content)
                got = []
                if not err and isinstance(obj, dict):
                    for p in obj.get("pairs", []):
                        try:
                            i, j = int(p["pred"]), int(p["truth"])
                        except (KeyError, TypeError, ValueError):
                            continue
                        if i in up and j in ut:
                            got.append((i, j))
                pairs[(model, sid, rep, typ)] = got
                rec.update(cost_usd=cost, latency_s=lat, pairs=got, raw=content)
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
    return pairs


class MockJudge:
    """Judge stand-in for --mock: never adds matches (keeps mock results deterministic)."""

    def chat(self, payload, meta=None):
        return ({"choices": [{"message": {"content": '{"pairs": []}'}, "finish_reason": "stop"}],
                 "usage": {"prompt_tokens": 0, "completion_tokens": 0}}, 0.0)
