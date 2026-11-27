"""Mock provider: answers from the ground truth with controlled noise.

It plugs in where the HTTP client would (same ``chat(payload, meta)`` interface and an
OpenAI-shaped response), so request building, parsing, format fallback, cost
accounting and scoring all run end to end without any network call.

Profiles are chosen by model name:

=========== ====== ======= ========= ======= ============ ===========================
profile     drop   extra   wrong     field   conf. sep.   notes
                           project   error
=========== ====== ======= ========= ======= ============ ===========================
mock-good   0.05   0.05    0.05      0.05    0.35         errors get clearly lower conf.
mock-noisy  0.20   0.15    0.20      0.15    0.20
mock-bad    0.45   0.35    0.45      0.30    0.00         confidence uninformative;
                                                          rejects json_schema (tests the
                                                          json_object fallback)
=========== ====== ======= ========= ======= ============ ===========================

Any other model name uses ``mock-noisy``. Latency is simulated (no sleeping).
"""

from __future__ import annotations

import hashlib
import json
import random

from .provider import ProviderError
from .text import approx_tokens

PROFILES = {
    "mock-good": dict(drop=0.05, extra=0.05, wrong=0.05, field=0.05, sep=0.35, reject_schema=False),
    "mock-noisy": dict(drop=0.20, extra=0.15, wrong=0.20, field=0.15, sep=0.20, reject_schema=False),
    "mock-bad": dict(drop=0.45, extra=0.35, wrong=0.45, field=0.30, sep=0.00, reject_schema=True),
}

DISTRACTORS = {
    "people": [{"name": "Roberto Galli", "role": None}, {"name": "Laura", "role": "consulente"}],
    "decisions": [{"text": "Rimandare la decisione alla prossima riunione"},
                  {"text": "Usare un foglio condiviso per tracciare le attività"}],
    "actions": [{"text": "Aggiornare la documentazione del progetto", "owner": "Io", "status": "open"},
                {"text": "Mandare il verbale ai partecipanti", "owner": None, "status": "open"}],
    "requirements": [{"text": "Il sistema deve essere sicuro"}, {"text": "Serve un ambiente di test"}],
    "open_questions": [{"text": "Quando si fa la prossima riunione?"}],
}

PREFIXES = ["", "", "Si decide di: ", "È stato concordato: ", "Nota: "]


def _rng(*parts) -> random.Random:
    h = hashlib.sha256("|".join(map(str, parts)).encode()).hexdigest()
    return random.Random(int(h[:16], 16))


def _conf(rng: random.Random, correct: bool, sep: float) -> float:
    mu = 0.85 if correct else 0.85 - sep
    return round(min(1.0, max(0.0, rng.gauss(mu, 0.1 if correct else 0.15))), 2)


def _paraphrase(rng: random.Random, text: str) -> str:
    words = text.split()
    if len(words) > 6 and rng.random() < 0.5:
        del words[rng.randrange(1, len(words) - 1)]   # drop one word
    return rng.choice(PREFIXES) + " ".join(words)


class MockClient:
    def __init__(self, sources: list[dict], project_ids: list[str]):
        self.sources = {s["id"]: s for s in sources}
        self.project_ids = project_ids

    def __repr__(self) -> str:
        return "MockClient()"

    def list_models(self) -> list[dict]:
        return [{"id": m, "object": "model"} for m in PROFILES]

    def chat(self, payload: dict, meta: dict | None = None) -> tuple[dict, float]:
        meta = meta or {}
        model = payload["model"]
        prof = PROFILES.get(model, PROFILES["mock-noisy"])
        rf = payload.get("response_format") or {}
        if prof["reject_schema"] and rf.get("type") == "json_schema":
            raise ProviderError("HTTP 400 on /chat/completions", status=400,
                                body='{"error":{"message":"response_format json_schema is not supported for this model"}}')
        src = self.sources[meta["source_id"]]
        rng = _rng(model, meta["source_id"], meta["task"], meta.get("repeat", 0))
        if meta["task"] == "assignment":
            out = self._assignment(rng, prof, src["truth"])
        else:
            out = self._extraction(rng, prof, src["truth"])
        content = json.dumps(out, ensure_ascii=False)
        if rf.get("type") != "json_schema" and rng.random() < 0.3:
            content = "```json\n" + content + "\n```"   # exercise the tolerant parser
        tin = sum(approx_tokens(m["content"]) for m in payload["messages"])
        if rf.get("type") == "json_schema":
            tin += approx_tokens(json.dumps(rf["json_schema"]["schema"]))
        tout = approx_tokens(content)
        latency = round(0.35 + tout * 0.012 + abs(rng.gauss(0, 0.3)), 3)
        resp = {
            "id": "mock", "object": "chat.completion", "model": model,
            "choices": [{"index": 0, "finish_reason": "stop",
                         "message": {"role": "assistant", "content": content}}],
            "usage": {"prompt_tokens": tin, "completion_tokens": tout, "total_tokens": tin + tout},
        }
        return resp, latency

    # ------------------------------------------------------------------ tasks

    def _assignment(self, rng, prof, truth):
        projects = list(truth["projects"])
        wrong = rng.random() < prof["wrong"]
        if wrong:
            choice = rng.random()
            others = [p for p in self.project_ids if p not in projects]
            if projects and choice < 0.4:
                projects = projects[:-1]                    # miss one
            elif others and choice < 0.8:
                projects = projects + [rng.choice(others)]  # add a wrong one
            elif others:
                projects = [rng.choice(others)]             # completely wrong
        dec_conf = _conf(rng, not wrong, prof["sep"])
        if truth.get("ambiguous") and not wrong:
            dec_conf = round(min(dec_conf, 0.6), 2)
        return {
            "assignments": [{"project_id": p, "confidence": _conf(rng, p in truth["projects"], prof["sep"]),
                             "reason": "mock"} for p in projects],
            "decision_confidence": dec_conf,
        }

    def _extraction(self, rng, prof, truth):
        out = {}
        names = [p["name"] for p in truth["entities"]["people"]]
        for typ, items in truth["entities"].items():
            res = []
            for t in items:
                if rng.random() < prof["drop"]:
                    continue
                field_err = rng.random() < prof["field"]
                if typ == "people":
                    name = t["aliases"][0] if t.get("aliases") and rng.random() < 0.3 else t["name"]
                    e = {"name": name, "role": t.get("role")}
                else:
                    e = {"text": _paraphrase(rng, t["text"])}
                    if typ == "decisions":
                        e.update(date=None if field_err else t.get("date"), reason=t.get("reason"),
                                 supersedes=t.get("supersedes"))
                    if typ == "actions":
                        owner, status = t.get("owner"), t["status"]
                        if field_err:
                            if rng.random() < 0.5:
                                status = "closed" if status == "open" else "open"
                            else:
                                alts = [n for n in names if n != owner] + ["Io"]
                                owner = rng.choice(alts)
                        e.update(owner=owner, status=status)
                ev = t.get("evidence", "")
                e["evidence"] = ev
                strict_bad = field_err and typ == "actions"
                e["confidence"] = _conf(rng, not strict_bad, prof["sep"])
                res.append(e)
            n_extra = sum(1 for _ in range(2) if rng.random() < prof["extra"])
            for _ in range(n_extra):
                d = dict(rng.choice(DISTRACTORS[typ]))
                d.setdefault("evidence", "")
                if typ == "decisions":
                    d.update(date=None, reason=None, supersedes=None)
                d["confidence"] = _conf(rng, False, prof["sep"])
                res.append(d)
            out[typ] = res
        return out
