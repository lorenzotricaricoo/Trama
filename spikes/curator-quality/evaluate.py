#!/usr/bin/env python3
"""Curator quality spike (Trama #12): evaluation CLI. Standard library only.

    python3 -I evaluate.py validate
    python3 -I evaluate.py estimate [--repeats 3]
    python3 -I evaluate.py run --mock --models mock-good,mock-noisy,mock-bad --repeats 2
    python3 -I evaluate.py catalog                     # needs CURATOR_BASE_URL + CURATOR_KEYCHAIN_SERVICE
    python3 -I evaluate.py run --models m1,m2 --repeats 3 --max-cost-usd 5
    python3 -I evaluate.py score out/<run-dir>

Provider settings come from flags or environment variables, so no endpoint needs to be
written in a committed file: CURATOR_BASE_URL, CURATOR_KEYCHAIN_SERVICE, CURATOR_MODELS.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))  # `python3 -I` does not add the script directory

from curator_eval import dataset, estimate, judge, runner, scoring  # noqa: E402
from curator_eval.mock import MockClient  # noqa: E402
from curator_eval.prompts import PromptKit  # noqa: E402
from curator_eval.provider import Budget, ChatClient, Pricing, read_keychain_secret  # noqa: E402

DATA = ROOT / "data"


def _csv(s):
    return [x.strip() for x in s.split(",") if x.strip()] if s else []


def add_provider_args(p):
    p.add_argument("--mock", action="store_true", help="use the mock provider (no network)")
    p.add_argument("--base-url", default=os.environ.get("CURATOR_BASE_URL"),
                   help="OpenAI-compatible base URL, e.g. https://host/v1 (env CURATOR_BASE_URL)")
    p.add_argument("--keychain-service", default=os.environ.get("CURATOR_KEYCHAIN_SERVICE"),
                   help="macOS Keychain service holding the API key (env CURATOR_KEYCHAIN_SERVICE)")
    p.add_argument("--no-auth", action="store_true", help="send no Authorization header (local servers)")
    p.add_argument("--timeout", type=float, default=180.0)


def make_client(args, sources, projects):
    if args.mock:
        return MockClient(sources, [p["id"] for p in projects])
    if not args.base_url:
        raise SystemExit("--base-url (or CURATOR_BASE_URL) is required unless --mock")
    key = None
    if not args.no_auth:
        if not args.keychain_service:
            raise SystemExit("--keychain-service (or CURATOR_KEYCHAIN_SERVICE) is required (or --no-auth)")
        key = read_keychain_secret(args.keychain_service)
    return ChatClient(args.base_url, key, timeout=args.timeout)


def print_catalog(client):
    models = client.list_models()
    for m in sorted(models, key=lambda x: str(x.get("id"))):
        extra = {k: v for k, v in m.items() if k in ("context_length", "context_window", "max_tokens",
                                                     "max_output_tokens", "pricing", "owned_by")}
        print(m.get("id"), json.dumps(extra, ensure_ascii=False) if extra else "")
    print(f"{len(models)} models")


def cmd_validate(args):
    problems = dataset.validate(DATA)
    st = dataset.stats(DATA)
    print(json.dumps(st, indent=2, ensure_ascii=False))
    if problems:
        print("\n".join(problems))
        raise SystemExit(f"{len(problems)} problem(s)")
    print("dataset OK")


def cmd_catalog(args):
    sources = dataset.load_sources(DATA)
    projects = dataset.load_projects(DATA)
    print_catalog(make_client(args, sources, projects))


def write_summary(out_dir, calls, sources, judge_pairs=None, note=""):
    scores = scoring.score_run(calls, sources, judge_pairs)
    md = scoring.markdown_summary(scores, note=note)
    (out_dir / "summary.md").write_text(md, encoding="utf-8")
    with open(out_dir / "scores.json", "w", encoding="utf-8") as f:
        json.dump(scores, f, indent=1, ensure_ascii=False, default=str)
    return md


def cmd_run(args):
    projects = dataset.load_projects(DATA)
    sources = dataset.load_sources(DATA, _csv(args.sources) or None)
    client = make_client(args, sources, projects)
    if args.models_from_catalog:
        print_catalog(client)
        return
    models = _csv(args.models or os.environ.get("CURATOR_MODELS", ""))
    if not models:
        raise SystemExit("--models is required")
    if not args.mock and args.max_cost_usd is None:
        raise SystemExit("--max-cost-usd is required for real runs")
    pricing = Pricing(Path(args.prices))
    for m in models:
        pin, pout, src = pricing.price(m)
        print(f"price {m}: in {pin} / out {pout} USD per 1M tokens ({src})")
    kit = PromptKit(ROOT, projects)
    out_dir = Path(args.out) if args.out else ROOT / "out" / time.strftime("run-%Y%m%d-%H%M%S")
    out_dir.mkdir(parents=True, exist_ok=True)
    meta = {"started": time.strftime("%Y-%m-%dT%H:%M:%S"), "models": models, "repeats": args.repeats,
            "sources": [s["id"] for s in sources], "mock": args.mock,
            "provider": "mock" if args.mock else "openai-compatible (endpoint not recorded)",
            "format": args.format, "temperature": args.temperature, "reasoning_effort": args.reasoning_effort,
            "max_tokens": {"assignment": args.max_tokens_assignment, "extraction": args.max_tokens_extraction},
            "max_cost_usd": args.max_cost_usd}
    with open(out_dir / "run.json", "a", encoding="utf-8") as f:
        f.write(json.dumps(meta) + "\n")
    print(f"output: {out_dir}")
    temp = None if args.temperature in (None, "none") else float(args.temperature)
    res = runner.run(client, sources, projects, kit, pricing, models, args.repeats, out_dir,
                     max_cost_usd=args.max_cost_usd, fmt=args.format,
                     max_tokens={"assignment": args.max_tokens_assignment,
                                 "extraction": args.max_tokens_extraction},
                     temperature=temp, reasoning_effort=args.reasoning_effort,
                     concurrency=args.concurrency, is_mock=args.mock)
    print(json.dumps(res, indent=1))
    calls = runner.load_calls(out_dir)
    judge_pairs = None
    if args.judge_model:
        jclient = judge.MockJudge() if args.mock else client
        budget = Budget(args.max_cost_usd, spent=res["spent_usd"])
        judge_pairs = judge.run_judge(jclient, args.judge_model, calls, sources, pricing, budget,
                                      out_dir / "judge.jsonl")
    note = ("**MOCK RUN** — answers generated from the ground truth with controlled noise; "
            "these numbers test the pipeline, they say nothing about real models.") if args.mock else ""
    print(write_summary(out_dir, calls, sources, judge_pairs, note))


def cmd_score(args):
    out_dir = Path(args.run_dir)
    sources = dataset.load_sources(DATA)
    calls = runner.load_calls(out_dir)
    if not calls:
        raise SystemExit(f"no calls.jsonl in {out_dir}")
    mock = any(c.get("mock") for c in calls)
    note = "**MOCK RUN** — pipeline test only." if mock else ""
    print(write_summary(out_dir, calls, sources, None, note))


def cmd_estimate(args):
    projects = dataset.load_projects(DATA)
    sources = dataset.load_sources(DATA)
    kit = PromptKit(ROOT, projects)
    pricing = Pricing(Path(args.prices))
    prof = estimate.token_profile(kit, sources)
    a, e = prof["assignment"], prof["extraction"]
    print(f"Sources: {prof['sources']}  (source text ≈ {prof['source_text']} tokens, approx tokenizer)")
    print(f"Assignment: input ≈ {a['in']} tokens, output ≈ {a['out']} tokens")
    print(f"Extraction: input ≈ {e['in']} tokens, output ≈ {e['out']} tokens")
    print(f"One full run (all sources, both tasks): input ≈ {a['in'] + e['in']}, output ≈ {a['out'] + e['out']}\n")
    print("| Price class / model | USD per 1M in / out | 1 run | 1 run, reasoning ×3 output | runs within budget |")
    print("|---|---|---|---|---|")
    rows = [(f"class: {k}", v["input"], v["output"]) for k, v in pricing.classes.items()]
    for m in _csv(args.models):
        pin, pout, _ = pricing.price(m)
        rows.append((m, pin, pout))
    for name, pin, pout in rows:
        c1 = estimate.run_cost(prof, pin, pout)
        c3 = estimate.run_cost(prof, pin, pout, 3.0)
        n = "∞" if c1 == 0 else f"{args.budget / c1:.0f}"
        print(f"| {name} | {pin} / {pout} | ${c1:.4f} | ${c3:.4f} | {n} |")
    if args.plan:
        total = total3 = 0.0
        print("\nPlan:")
        for item in _csv(args.plan):
            name, reps = item.rsplit(":", 1)
            if name in pricing.classes:
                pin, pout = pricing.classes[name]["input"], pricing.classes[name]["output"]
            else:
                pin, pout, _ = pricing.price(name)
            c = estimate.run_cost(prof, pin, pout) * int(reps)
            c3 = estimate.run_cost(prof, pin, pout, 3.0) * int(reps)
            total += c
            total3 += c3
            print(f"  {name} × {reps} repeats: ${c:.3f} (reasoning ×3: ${c3:.3f})")
        print(f"  TOTAL ${total:.3f} (reasoning ×3: ${total3:.3f}) of a {args.budget} USD budget")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("validate", help="check the integrity of the test set").set_defaults(fn=cmd_validate)

    p = sub.add_parser("catalog", help="list the provider's /models")
    add_provider_args(p)
    p.set_defaults(fn=cmd_catalog)

    p = sub.add_parser("run", help="call the models and score them")
    add_provider_args(p)
    p.add_argument("--models", help="comma-separated model ids (env CURATOR_MODELS)")
    p.add_argument("--models-from-catalog", action="store_true", help="list the provider's /models and exit")
    p.add_argument("--repeats", type=int, default=1)
    p.add_argument("--max-cost-usd", type=float, default=None, help="hard stop on the running cost estimate")
    p.add_argument("--prices", default=str(ROOT / "prices.json"))
    p.add_argument("--sources", help="subset of source ids, e.g. S01,S07")
    p.add_argument("--out", help="output dir (default out/run-<timestamp>, gitignored); reuse to resume")
    p.add_argument("--format", choices=["json_schema", "json_object", "none"], default="json_schema",
                   help="starting response_format; falls back automatically on HTTP 400")
    p.add_argument("--temperature", default="0", help="number, or 'none' to omit")
    p.add_argument("--reasoning-effort", default=None, help="passed through if set (e.g. low)")
    p.add_argument("--max-tokens-assignment", type=int, default=600)
    p.add_argument("--max-tokens-extraction", type=int, default=4000)
    p.add_argument("--concurrency", type=int, default=4)
    p.add_argument("--judge-model", default=None, help="enable the optional LLM judge (off by default)")
    p.set_defaults(fn=cmd_run)

    p = sub.add_parser("score", help="re-score a recorded run without calling anything")
    p.add_argument("run_dir")
    p.set_defaults(fn=cmd_score)

    p = sub.add_parser("estimate", help="token and cost estimate, no calls")
    p.add_argument("--prices", default=str(ROOT / "prices.json"))
    p.add_argument("--models", help="also price these model ids from prices.json")
    p.add_argument("--plan", help="e.g. nano:3,mini:3,strong:2 or model-id:3,...")
    p.add_argument("--budget", type=float, default=5.0)
    p.set_defaults(fn=cmd_estimate)

    args = ap.parse_args(argv)
    args.fn(args)


if __name__ == "__main__":
    main()
