"""Loading and validating the synthetic test set."""

from __future__ import annotations

import json
import re
from pathlib import Path

from .text import normalize

ENTITY_TYPES = ("people", "decisions", "actions", "requirements", "open_questions")
# open_questions are optional in the spike brief: they are scored but reported apart
# and excluded from the "core" averages.
CORE_TYPES = ("people", "decisions", "actions", "requirements")


def load_projects(data_dir: Path) -> list[dict]:
    with open(data_dir / "projects.json", encoding="utf-8") as f:
        return json.load(f)["projects"]


def load_sources(data_dir: Path, only: list[str] | None = None) -> list[dict]:
    """Return one dict per source: id, type, date, text and its ground truth."""
    out = []
    for path in sorted((data_dir / "truth").glob("S*.json")):
        with open(path, encoding="utf-8") as f:
            truth = json.load(f)
        if only and truth["id"] not in only:
            continue
        text = (data_dir / truth["file"]).read_text(encoding="utf-8")
        for t in ENTITY_TYPES:
            truth["entities"].setdefault(t, [])
        out.append({"id": truth["id"], "type": truth["type"], "date": truth["date"],
                    "text": text, "truth": truth})
    if only:
        missing = set(only) - {s["id"] for s in out}
        if missing:
            raise SystemExit(f"unknown source ids: {sorted(missing)}")
    return out


def roster(truth: dict) -> list[str]:
    """Names of the people of one source (used to disambiguate first names)."""
    names = [p["name"] for p in truth["entities"]["people"]]
    for a in truth["entities"]["actions"]:
        o = a.get("owner")
        if o and o != "Io" and o not in names:
            names.append(o)
    return names


def _squash(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def validate(data_dir: Path) -> list[str]:
    """Integrity checks: every evidence is a verbatim quote of its source, project ids
    exist, action owners are people of the source (or 'Io' / null), required fields."""
    problems = []
    project_ids = {p["id"] for p in load_projects(data_dir)}
    for s in load_sources(data_dir):
        t, sid = s["truth"], s["id"]
        text = _squash(s["text"])
        for pid in t["projects"]:
            if pid not in project_ids:
                problems.append(f"{sid}: unknown project {pid}")
        for alt in t.get("acceptable_projects", []):
            for pid in alt:
                if pid not in project_ids:
                    problems.append(f"{sid}: unknown project {pid} in acceptable_projects")
        names = {p["name"] for p in t["entities"]["people"]}
        for typ in ENTITY_TYPES:
            for i, e in enumerate(t["entities"][typ]):
                ev = e.get("evidence")
                if not ev:
                    problems.append(f"{sid}/{typ}[{i}]: missing evidence")
                elif _squash(ev) not in text:
                    problems.append(f"{sid}/{typ}[{i}]: evidence not found verbatim: {ev!r}")
                key = "name" if typ == "people" else "text"
                if not e.get(key):
                    problems.append(f"{sid}/{typ}[{i}]: missing {key}")
                if typ != "people" and not e.get("keywords"):
                    problems.append(f"{sid}/{typ}[{i}]: no keywords")
                for kw in e.get("keywords", []):
                    if not normalize(kw):
                        problems.append(f"{sid}/{typ}[{i}]: empty keyword")
                if typ == "actions":
                    if e.get("status") not in ("open", "closed"):
                        problems.append(f"{sid}/actions[{i}]: bad status")
                    o = e.get("owner")
                    if o not in (None, "Io") and o not in names:
                        problems.append(f"{sid}/actions[{i}]: owner {o!r} not among people")
                if typ == "decisions" and e.get("date") and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", e["date"]):
                    problems.append(f"{sid}/decisions[{i}]: bad date")
    return problems


def stats(data_dir: Path) -> dict:
    srcs = load_sources(data_dir)
    out = {"sources": len(srcs), "by_type": {}, "by_projects": {}, "entities": {}}
    for s in srcs:
        out["by_type"][s["type"]] = out["by_type"].get(s["type"], 0) + 1
        k = "+".join(s["truth"]["projects"]) or "(none)"
        out["by_projects"][k] = out["by_projects"].get(k, 0) + 1
        for typ in ENTITY_TYPES:
            ents = s["truth"]["entities"][typ]
            d = out["entities"].setdefault(typ, {"scored": 0, "optional": 0})
            for e in ents:
                d["optional" if e.get("optional") else "scored"] += 1
    return out
