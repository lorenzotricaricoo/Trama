"""Metrics, dataset integrity and end-to-end runs (mock provider and a local fake
OpenAI-compatible HTTP server on 127.0.0.1). No external network."""

import json
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from curator_eval import dataset, runner, scoring  # noqa: E402
from curator_eval.mock import MockClient  # noqa: E402
from curator_eval.prompts import PromptKit, coerce, parse_json_content  # noqa: E402
from curator_eval.provider import Budget, BudgetExceeded, ChatClient, Pricing  # noqa: E402

DATA = ROOT / "data"


def quiet(*a, **k):
    pass


class MetricsTests(unittest.TestCase):
    def test_score_assignment(self):
        truth = {"projects": ["faro", "ghianda"]}
        r = scoring.score_assignment({"assignments": [{"project_id": "faro", "confidence": 0.9}],
                                      "decision_confidence": 0.5}, truth)
        self.assertFalse(r["exact"])
        self.assertAlmostEqual(r["jaccard"], 0.5)
        self.assertEqual((r["tp"], r["fp"], r["fn"]), (1, 0, 1))
        none = scoring.score_assignment({"assignments": [], "decision_confidence": 0.8}, {"projects": []})
        self.assertTrue(none["exact"])
        self.assertEqual(none["jaccard"], 1.0)
        failed = scoring.score_assignment(None, {"projects": []})
        self.assertFalse(failed["exact"])
        self.assertEqual(failed["conf"], 0.0)

    def test_acceptable_alternatives(self):
        truth = {"projects": [], "acceptable_projects": [[], ["marea"]]}
        r = scoring.score_assignment({"assignments": [{"project_id": "marea", "confidence": 0.4}],
                                      "decision_confidence": 0.4}, truth)
        self.assertTrue(r["exact"])

    def test_confidence_table(self):
        pairs = [(0.95, True), (0.9, True), (0.55, False), (0.85, False), (None, False)]
        rows = {r["threshold"]: r for r in scoring.confidence_table(pairs)}
        self.assertAlmostEqual(rows[0.6]["errors_caught"], 0.5)
        self.assertAlmostEqual(rows[0.6]["correct_to_review"], 0.0)
        self.assertAlmostEqual(rows[0.6]["review_load"], 0.25)
        self.assertAlmostEqual(rows[0.9]["errors_caught"], 1.0)
        self.assertAlmostEqual(rows[0.9]["correct_to_review"], 0.0)

    def test_percentile(self):
        self.assertIsNone(scoring.percentile([], 0.5))
        self.assertEqual(scoring.percentile([3, 1, 2], 0.5), 2)
        self.assertAlmostEqual(scoring.percentile([1, 2, 3, 4], 0.95), 3.85)


class ParsingTests(unittest.TestCase):
    def test_parse_fenced_and_prose(self):
        self.assertEqual(parse_json_content('```json\n{"a": 1}\n```')[0], {"a": 1})
        self.assertEqual(parse_json_content('Ecco: {"a": 2} fine')[0], {"a": 2})
        self.assertIsNotNone(parse_json_content("niente")[1])

    def test_coerce_assignment(self):
        out, w = coerce("assignment", {"assignments": [{"project_id": "faro", "confidence": 1.7},
                                                       {"project_id": "boh", "confidence": 0.2}],
                                       "decision_confidence": "0.8"}, {"faro"})
        self.assertEqual(out["assignments"][0]["confidence"], 1.0)
        self.assertEqual(len(out["assignments"]), 1)
        self.assertEqual(out["decision_confidence"], 0.8)
        self.assertTrue(w)


class DatasetTests(unittest.TestCase):
    def test_dataset_valid(self):
        self.assertEqual(dataset.validate(DATA), [])

    def test_hard_cases_present(self):
        srcs = dataset.load_sources(DATA)
        self.assertGreaterEqual(len(srcs), 15)
        self.assertTrue(any(not s["truth"]["projects"] for s in srcs))
        self.assertTrue(any(len(s["truth"]["projects"]) > 1 for s in srcs))
        self.assertTrue(any(s["truth"].get("ambiguous") for s in srcs))
        sup = [d for s in srcs for d in s["truth"]["entities"]["decisions"] if d.get("supersedes")]
        self.assertGreaterEqual(len(sup), 2)
        closed = [a for s in srcs for a in s["truth"]["entities"]["actions"] if a["status"] == "closed"]
        self.assertGreaterEqual(len(closed), 5)


class BudgetTests(unittest.TestCase):
    def test_budget_cap(self):
        b = Budget(1.0)
        b.reserve(0.6)
        b.settle(0.6, 0.5)
        with self.assertRaises(BudgetExceeded):
            b.reserve(0.6)
        b.reserve(0.4)


class MockRunTests(unittest.TestCase):
    def test_end_to_end_mock_and_fallback(self):
        projects = dataset.load_projects(DATA)
        sources = dataset.load_sources(DATA, ["S01", "S08", "S11"])
        kit = PromptKit(ROOT, projects)
        pricing = Pricing(ROOT / "prices.json")
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            res = runner.run(MockClient(sources, [p["id"] for p in projects]), sources, projects, kit,
                             pricing, ["mock-good", "mock-bad"], 2, out, is_mock=True, log=quiet)
            self.assertEqual(res["errors"], 0)
            self.assertEqual(res["formats"]["mock-bad"], "json_object")  # fell back after a 400
            calls = runner.load_calls(out)
            self.assertEqual(len(calls), 3 * 2 * 2 * 2)
            scores = scoring.score_run(calls, sources)
            self.assertGreater(scores["mock-good"]["agg"]["core_f1"][0], scores["mock-bad"]["agg"]["core_f1"][0])
            md = scoring.markdown_summary(scores)
            self.assertIn("| mock-good |", md)
            # resume: nothing left to do
            res2 = runner.run(MockClient(sources, [p["id"] for p in projects]), sources, projects, kit,
                              pricing, ["mock-good", "mock-bad"], 2, out, is_mock=True, log=quiet)
            self.assertEqual(res2["ok"] + res2["errors"], 0)

    def test_budget_stop(self):
        projects = dataset.load_projects(DATA)
        sources = dataset.load_sources(DATA, ["S01", "S02", "S03"])
        kit = PromptKit(ROOT, projects)
        pricing = Pricing(ROOT / "prices.json")
        with tempfile.TemporaryDirectory() as d:
            res = runner.run(MockClient(sources, [p["id"] for p in projects]), sources, projects, kit,
                             pricing, ["claude-opus-5-5"], 3, Path(d), max_cost_usd=0.05,
                             is_mock=True, concurrency=1, log=quiet)
            self.assertTrue(res["stopped"])
            self.assertLessEqual(res["spent_usd"], 0.05)


class FakeOpenAI(BaseHTTPRequestHandler):
    """Rejects json_schema and max_tokens like some real endpoints, then answers."""
    seen = []

    def log_message(self, *a):
        pass

    def _send(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._send(200, {"data": [{"id": "fake-model", "object": "model"}]})

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        req = json.loads(self.rfile.read(n))
        FakeOpenAI.seen.append({"auth": self.headers.get("Authorization"), "keys": sorted(req)})
        if req.get("response_format", {}).get("type") == "json_schema":
            return self._send(400, {"error": {"message": "response_format.json_schema is not supported"}})
        if "max_tokens" in req:
            return self._send(400, {"error": {"message": "Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens'."}})
        system = req["messages"][0]["content"]
        if "which of the user's projects" in system:
            content = {"assignments": [{"project_id": "faro", "confidence": 0.9, "reason": "x"}],
                       "decision_confidence": 0.85}
        else:
            content = {"people": [{"name": "Elena Sartori", "role": "IT", "evidence": "", "confidence": 0.9}],
                       "decisions": [], "actions": [], "requirements": [], "open_questions": []}
        self._send(200, {"choices": [{"index": 0, "finish_reason": "stop",
                                      "message": {"role": "assistant", "content": json.dumps(content)}}],
                         "usage": {"prompt_tokens": 1000, "completion_tokens": 100,
                                   "completion_tokens_details": {"reasoning_tokens": 20}}})


class HttpClientTests(unittest.TestCase):
    def test_http_client_with_adaptation(self):
        srv = HTTPServer(("127.0.0.1", 0), FakeOpenAI)
        t = threading.Thread(target=srv.serve_forever, daemon=True)
        t.start()
        try:
            client = ChatClient(f"http://127.0.0.1:{srv.server_port}/v1", "test-key-not-secret", timeout=10)
            self.assertNotIn("test-key", repr(client))
            self.assertEqual(client.list_models()[0]["id"], "fake-model")
            projects = dataset.load_projects(DATA)
            sources = dataset.load_sources(DATA, ["S01"])
            kit = PromptKit(ROOT, projects)
            pricing = Pricing(ROOT / "prices.json")
            with tempfile.TemporaryDirectory() as d:
                res = runner.run(client, sources, projects, kit, pricing, ["gpt-5-nano"], 1, Path(d),
                                 max_cost_usd=1.0, concurrency=1, log=quiet)
                self.assertEqual(res["errors"], 0)
                self.assertEqual(res["formats"]["gpt-5-nano"], "json_object")
                self.assertEqual(res["token_param"]["gpt-5-nano"], "max_completion_tokens")
                calls = runner.load_calls(Path(d))
                self.assertTrue(all(c["ok"] for c in calls))
                self.assertEqual(calls[0]["reasoning_tokens"], 20)
                self.assertAlmostEqual(calls[0]["cost_usd"], (1000 * 0.055 + 100 * 0.44) / 1e6)
                raw = (Path(d) / "calls.jsonl").read_text()
                self.assertNotIn("test-key-not-secret", raw)
            self.assertTrue(all(s["auth"] == "Bearer test-key-not-secret" for s in FakeOpenAI.seen))
        finally:
            srv.shutdown()
            srv.server_close()


if __name__ == "__main__":
    unittest.main()
