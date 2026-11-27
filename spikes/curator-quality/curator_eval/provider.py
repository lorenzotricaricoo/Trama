"""OpenAI-compatible HTTP client (urllib), API key from the macOS Keychain, pricing
and the budget guard.

The API key is read once with ``security find-generic-password -s NAME -w``, kept in
memory only, sent only in the Authorization header and never printed, logged or
written anywhere. Error messages never include request headers.
"""

from __future__ import annotations

import json
import subprocess
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


class ProviderError(Exception):
    def __init__(self, message: str, status: int | None = None, body: str = ""):
        super().__init__(message)
        self.status = status
        self.body = body[:2000]


class BudgetExceeded(Exception):
    pass


def read_keychain_secret(service: str) -> str:
    try:
        proc = subprocess.run(["security", "find-generic-password", "-s", service, "-w"],
                              capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise SystemExit(f"cannot run 'security' to read Keychain item {service!r}: {type(e).__name__}")
    if proc.returncode != 0:
        # stderr of `security` does not contain the secret; stdout is never shown.
        raise SystemExit(f"Keychain item {service!r} not found or not readable (security exit {proc.returncode})")
    key = proc.stdout.strip()
    if not key:
        raise SystemExit(f"Keychain item {service!r} is empty")
    return key


class ChatClient:
    """Minimal OpenAI-compatible client: GET /models, POST /chat/completions."""

    def __init__(self, base_url: str, api_key: str | None, timeout: float = 180.0,
                 max_retries: int = 4, user_agent: str = "curator-quality-spike/1.0"):
        self.base_url = base_url.rstrip("/")
        self._api_key = api_key
        self.timeout = timeout
        self.max_retries = max_retries
        self.user_agent = user_agent

    def __repr__(self) -> str:  # never expose the key
        return f"ChatClient(base_url=<hidden>, key={'set' if self._api_key else 'none'})"

    def _request(self, method: str, path: str, payload: dict | None = None) -> dict:
        data = json.dumps(payload).encode("utf-8") if payload is not None else None
        headers = {"Content-Type": "application/json", "Accept": "application/json",
                   "User-Agent": self.user_agent}
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"
        attempt = 0
        while True:
            attempt += 1
            req = urllib.request.Request(self.base_url + path, data=data, method=method, headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except urllib.error.HTTPError as e:
                body = ""
                try:
                    body = e.read().decode("utf-8", "replace")
                except Exception:
                    pass
                finally:
                    e.close()
                retriable = e.code in (408, 409, 429, 500, 502, 503, 504)
                if retriable and attempt <= self.max_retries:
                    ra = e.headers.get("Retry-After") if e.headers else None
                    try:
                        wait = float(ra) if ra else 2.0 * 2 ** (attempt - 1)
                    except ValueError:
                        wait = 2.0 * 2 ** (attempt - 1)
                    time.sleep(min(wait, 60.0))
                    continue
                raise ProviderError(f"HTTP {e.code} on {path}", status=e.code, body=body) from None
            except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
                if attempt <= self.max_retries:
                    time.sleep(2.0 * 2 ** (attempt - 1))
                    continue
                raise ProviderError(f"network error on {path}: {type(e).__name__}") from None
            except json.JSONDecodeError:
                raise ProviderError(f"non-JSON response on {path}") from None

    def list_models(self) -> list[dict]:
        resp = self._request("GET", "/models")
        return resp.get("data", resp if isinstance(resp, list) else [])

    def chat(self, payload: dict, meta: dict | None = None) -> tuple[dict, float]:
        t0 = time.perf_counter()
        resp = self._request("POST", "/chat/completions", payload)
        return resp, time.perf_counter() - t0


# --------------------------------------------------------------------------- pricing

class Pricing:
    def __init__(self, path: Path):
        with open(path, encoding="utf-8") as f:
            cfg = json.load(f)
        self.classes = cfg["classes"]
        self.models = cfg.get("models", {})
        self.fallback = cfg.get("fallback_class", "strong")

    def price(self, model: str) -> tuple[float, float, str]:
        """(USD per 1M input, USD per 1M output, where the price came from)."""
        m = self.models.get(model)
        if m and "input" in m:
            return float(m["input"]), float(m["output"]), "model"
        if m and m.get("class") in self.classes:
            c = self.classes[m["class"]]
            return float(c["input"]), float(c["output"]), f"class:{m['class']}"
        c = self.classes[self.fallback]
        return float(c["input"]), float(c["output"]), f"fallback:{self.fallback}"

    def cost(self, model: str, tokens_in: int, tokens_out: int) -> float:
        pin, pout, _ = self.price(model)
        return (tokens_in * pin + tokens_out * pout) / 1e6


class Budget:
    """Thread-safe running cost with a hard cap.

    Before each call the worst case (estimated input + max output tokens) is reserved;
    if spent + reserved + worst case would exceed the cap the call is refused. After the
    call the reservation is replaced by the actual cost.
    """

    def __init__(self, max_usd: float | None, spent: float = 0.0):
        self.max_usd = max_usd
        self.spent = spent
        self.reserved = 0.0
        self._lock = threading.Lock()

    def reserve(self, worst_case: float) -> None:
        with self._lock:
            if self.max_usd is not None and self.spent + self.reserved + worst_case > self.max_usd:
                raise BudgetExceeded(
                    f"budget cap {self.max_usd:.2f} USD would be exceeded "
                    f"(spent {self.spent:.4f}, in flight {self.reserved:.4f}, next call up to {worst_case:.4f})")
            self.reserved += worst_case

    def settle(self, worst_case: float, actual: float) -> None:
        with self._lock:
            self.reserved -= worst_case
            self.spent += actual
