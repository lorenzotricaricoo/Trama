"""Prompt and response_format construction for the two curator tasks."""

from __future__ import annotations

import copy
import json
from pathlib import Path

TASKS = ("assignment", "extraction")
FORMATS = ("json_schema", "json_object", "none")


class PromptKit:
    def __init__(self, root: Path, projects: list[dict]):
        self.projects = projects
        self.templates = {t: (root / "prompts" / f"{t}.txt").read_text(encoding="utf-8") for t in TASKS}
        self.schemas = {}
        for t in TASKS:
            with open(root / "schemas" / f"{t}.schema.json", encoding="utf-8") as f:
                self.schemas[t] = json.load(f)
        ids = [p["id"] for p in projects]
        item = self.schemas["assignment"]["properties"]["assignments"]["items"]
        item["properties"]["project_id"]["enum"] = ids

    def project_block(self) -> str:
        return "\n".join(f"- {p['id']}: {p['name']} — {p['description']}" for p in self.projects)

    def schema(self, task: str) -> dict:
        return copy.deepcopy(self.schemas[task])

    def messages(self, task: str, source: dict, fmt: str) -> list[dict]:
        if fmt == "json_schema":
            hint = "\nRespond with the JSON object only."
        else:
            hint = ("\nRespond with a single JSON object (no prose, no code fences) that follows "
                    "this JSON schema exactly:\n" + json.dumps(self.schemas[task], ensure_ascii=False))
        system = self.templates[task].replace("{format_hint}", hint)
        if task == "assignment":
            system = system.replace("{projects}", self.project_block())
        user = (f"SOURCE (type: {source['type']}, date: {source['date']})\n"
                f"-----\n{source['text'].strip()}\n-----")
        return [{"role": "system", "content": system}, {"role": "user", "content": user}]

    def response_format(self, task: str, fmt: str) -> dict | None:
        if fmt == "json_schema":
            return {"type": "json_schema",
                    "json_schema": {"name": f"curator_{task}", "strict": True, "schema": self.schema(task)}}
        if fmt == "json_object":
            return {"type": "json_object"}
        return None


def parse_json_content(content: str | None):
    """Parse a model reply into JSON. Tolerates code fences and leading/trailing prose.
    Returns (obj, error)."""
    if content is None:
        return None, "empty content"
    s = content.strip()
    if s.startswith("```"):
        s = s.split("\n", 1)[1] if "\n" in s else s[3:]
        if s.rstrip().endswith("```"):
            s = s.rstrip()[:-3]
    try:
        return json.loads(s), None
    except json.JSONDecodeError:
        a, b = s.find("{"), s.rfind("}")
        if a != -1 and b > a:
            try:
                return json.loads(s[a:b + 1]), None
            except json.JSONDecodeError as e:
                return None, f"invalid JSON: {e}"
        return None, "no JSON object found"


def _num(x):
    if isinstance(x, bool):
        return None
    if isinstance(x, (int, float)):
        return min(1.0, max(0.0, float(x)))
    try:
        return min(1.0, max(0.0, float(x)))
    except (TypeError, ValueError):
        return None


def coerce(task: str, obj, project_ids: set[str]) -> tuple[dict, list[str]]:
    """Bring a parsed reply to the expected shape; report what had to be fixed."""
    warnings = []
    if not isinstance(obj, dict):
        return ({"assignments": [], "decision_confidence": None} if task == "assignment"
                else {k: [] for k in ("people", "decisions", "actions", "requirements", "open_questions")},
                ["reply is not an object"])
    if task == "assignment":
        out, seen = [], set()
        for a in obj.get("assignments") or []:
            if not isinstance(a, dict):
                warnings.append("non-object assignment")
                continue
            pid = str(a.get("project_id", "")).strip()
            if pid not in project_ids:
                warnings.append(f"unknown project id {pid!r}")
                continue
            if pid in seen:
                continue
            seen.add(pid)
            out.append({"project_id": pid, "confidence": _num(a.get("confidence")), "reason": a.get("reason")})
        return {"assignments": out, "decision_confidence": _num(obj.get("decision_confidence"))}, warnings
    res = {}
    for k in ("people", "decisions", "actions", "requirements", "open_questions"):
        items = obj.get(k)
        if items is None:
            warnings.append(f"missing {k}")
            items = []
        clean = []
        for e in items if isinstance(items, list) else []:
            if not isinstance(e, dict):
                warnings.append(f"non-object item in {k}")
                continue
            e = dict(e)
            e["confidence"] = _num(e.get("confidence"))
            if k == "actions" and e.get("status") not in ("open", "closed"):
                st = str(e.get("status") or "").lower()
                e["status"] = "closed" if st in ("done", "completed", "chiusa", "chiuso", "fatto") else "open"
                warnings.append("action status coerced")
            clean.append(e)
        res[k] = clean
    return res, warnings
