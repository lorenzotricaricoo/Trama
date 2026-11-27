"""Runs the two curator tasks for every (model, source, repeat) and records each call
to ``calls.jsonl`` in the output directory. Resumable: calls already recorded as
successful are skipped when the same output directory is reused."""

from __future__ import annotations

import json
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from .prompts import FORMATS, PromptKit, coerce, parse_json_content
from .provider import Budget, BudgetExceeded, Pricing, ProviderError
from .text import approx_tokens

MAX_ADAPT = 5


class ModelState:
    """What a model/endpoint accepts, learned from 400 errors and reused."""

    def __init__(self, fmt: str, token_param: str, temperature, reasoning_effort):
        self.fmt = fmt
        self.token_param = token_param
        self.temperature = temperature
        self.reasoning_effort = reasoning_effort
        self.lock = threading.Lock()

    def adapt(self, body: str) -> str | None:
        """Change one setting based on a 400 body; return what changed or None."""
        b = body.lower()
        with self.lock:
            if ("response_format" in b or "json_schema" in b or "structured" in b or "schema" in b) \
                    and self.fmt != "none":
                self.fmt = FORMATS[FORMATS.index(self.fmt) + 1]
                return f"format->{self.fmt}"
            if "max_tokens" in b and self.token_param == "max_tokens":
                self.token_param = "max_completion_tokens"
                return "token_param->max_completion_tokens"
            if "max_completion_tokens" in b and self.token_param == "max_completion_tokens":
                self.token_param = "max_tokens"
                return "token_param->max_tokens"
            if "temperature" in b and self.temperature is not None:
                self.temperature = None
                return "temperature->omitted"
            if "reasoning" in b and self.reasoning_effort is not None:
                self.reasoning_effort = None
                return "reasoning_effort->omitted"
        return None


def _key(rec: dict) -> tuple:
    return (rec["model"], rec["source_id"], rec["repeat"], rec["task"])


def load_calls(out_dir: Path) -> list[dict]:
    p = out_dir / "calls.jsonl"
    if not p.exists():
        return []
    with open(p, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def run(client, sources: list[dict], projects: list[dict], kit: PromptKit, pricing: Pricing,
        models: list[str], repeats: int, out_dir: Path, *, tasks=("assignment", "extraction"),
        max_cost_usd: float | None = None, fmt: str = "json_schema", max_tokens: dict | None = None,
        temperature: float | None = 0.0, reasoning_effort: str | None = None,
        concurrency: int = 4, is_mock: bool = False, log=print) -> dict:
    out_dir.mkdir(parents=True, exist_ok=True)
    max_tokens = max_tokens or {"assignment": 600, "extraction": 4000}
    previous = load_calls(out_dir)
    done = {_key(r) for r in previous if r.get("ok")}
    spent_before = sum(r.get("cost_usd") or 0.0 for r in previous)
    budget = Budget(max_cost_usd, spent=spent_before)
    project_ids = {p["id"] for p in projects}
    states = {m: ModelState(fmt, "max_tokens", temperature, reasoning_effort) for m in models}
    write_lock = threading.Lock()
    stop = threading.Event()
    calls_path = out_dir / "calls.jsonl"

    jobs = [(m, s, r, t) for r in range(repeats) for s in sources for m in models for t in tasks
            if (m, s["id"], r, t) not in done]
    log(f"{len(jobs)} calls to make ({len(done)} already done); budget spent so far {spent_before:.4f} USD")

    def one(job):
        model, src, rep, task = job
        if stop.is_set():
            return None
        st = states[model]
        rec = {"model": model, "source_id": src["id"], "repeat": rep, "task": task, "ok": False,
               "mock": is_mock, "ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "adaptations": [],
               "attempts": 0}
        mt = max_tokens[task]
        resp = None
        latency = 0.0
        while rec["attempts"] < MAX_ADAPT + 2:
            rec["attempts"] += 1
            cur_fmt = st.fmt
            messages = kit.messages(task, src, cur_fmt)
            payload = {"model": model, "messages": messages, st.token_param: mt}
            rf = kit.response_format(task, cur_fmt)
            if rf:
                payload["response_format"] = rf
            if st.temperature is not None:
                payload["temperature"] = st.temperature
            if st.reasoning_effort:
                payload["reasoning_effort"] = st.reasoning_effort
            est_in = sum(approx_tokens(m["content"]) for m in messages)
            if rf and rf["type"] == "json_schema":
                est_in += approx_tokens(json.dumps(rf["json_schema"]["schema"]))
            worst = pricing.cost(model, int(est_in * 1.2) + 50, mt)
            try:
                budget.reserve(worst)
            except BudgetExceeded as e:
                stop.set()
                log(f"STOP: {e}")
                return None
            try:
                resp, latency = client.chat(payload, meta={"source_id": src["id"], "task": task, "repeat": rep})
            except ProviderError as e:
                budget.settle(worst, 0.0)
                change = st.adapt(e.body) if e.status == 400 else None
                if change:
                    rec["adaptations"].append(change)
                    continue
                rec.update(error=str(e), error_body=e.body[:500], format=cur_fmt)
                break
            usage = resp.get("usage") or {}
            tin = usage.get("prompt_tokens")
            tout = usage.get("completion_tokens")
            rec["usage_estimated"] = tin is None or tout is None
            content = None
            finish = None
            try:
                ch = resp["choices"][0]
                content = ch["message"].get("content")
                finish = ch.get("finish_reason")
            except (KeyError, IndexError, TypeError):
                pass
            if tin is None:
                tin = est_in
            if tout is None:
                tout = approx_tokens(content or "")
            cost = pricing.cost(model, tin, tout)
            budget.settle(worst, cost)
            details = usage.get("completion_tokens_details") or {}
            rec.update(format=cur_fmt, tokens_in=tin, tokens_out=tout,
                       reasoning_tokens=details.get("reasoning_tokens"),
                       cost_usd=round(cost, 8), latency_s=round(latency, 3), finish_reason=finish,
                       max_tokens=mt, raw=content, est_tokens_in=est_in)
            rec["cost_accum"] = rec.get("cost_accum", 0.0) + cost
            rec["latency_accum"] = rec.get("latency_accum", 0.0) + latency
            if finish == "length" and mt < 16000:
                rec["adaptations"].append(f"truncated at {mt}, retry with {mt * 2}")
                mt *= 2
                continue
            if finish == "length":
                rec["error"] = "truncated (finish_reason=length)"
                break
            obj, err = parse_json_content(content)
            if err:
                rec["error"] = f"parse: {err}"
                break
            parsed, warnings = coerce(task, obj, project_ids)
            rec.update(ok=True, parsed=parsed, warnings=warnings)
            break
        # total cost/latency of the job include truncated attempts
        if "cost_accum" in rec:
            rec["cost_usd"] = round(rec.pop("cost_accum"), 8)
            rec["latency_s"] = round(rec.pop("latency_accum"), 3)
        with write_lock:
            with open(calls_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        return rec

    n_ok = n_err = 0
    with ThreadPoolExecutor(max_workers=max(1, concurrency)) as ex:
        futures = [ex.submit(one, j) for j in jobs]
        for i, fut in enumerate(as_completed(futures), 1):
            rec = fut.result()
            if rec is None:
                continue
            if rec["ok"]:
                n_ok += 1
            else:
                n_err += 1
                log(f"  error {rec['model']} {rec['source_id']} r{rec['repeat']} {rec['task']}: {rec.get('error')}")
            if i % 20 == 0 or i == len(futures):
                log(f"  {i}/{len(futures)} calls, spent {budget.spent:.4f} USD")
                sys.stdout.flush()
    return {"ok": n_ok, "errors": n_err, "spent_usd": budget.spent, "stopped": stop.is_set(),
            "formats": {m: s.fmt for m, s in states.items()},
            "token_param": {m: s.token_param for m, s in states.items()}}
