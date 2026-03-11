#!/usr/bin/env python3
"""
Isartor AI Gateway — Layer Performance Benchmark

Evaluates end-to-end latency and throughput across the four serving layers:

  Layer 1a (Exact Cache)    — Repeat the *exact same* prompt; hits SHA-256 cache.
  Layer 1b (Semantic Cache) — Paraphrased prompts; hits the vector/cosine cache.
  Layer 2  (SLM)            — Simple prompts the embedded model handles locally.
  Layer 3  (Cloud)          — Complex prompts forwarded to the external LLM.

Usage:
  python tests/perf_layers.py [URL] [--rounds N] [--concurrency C] [--api-key KEY]

Requires: pip install requests
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field

try:
    import requests
except ImportError:
    print("Error: `requests` is required.  Install with:  pip install requests")
    sys.exit(1)


# ── Test prompts per layer ──────────────────────────────────────────

# L1a exact cache: first request primes the cache, subsequent ones
# with the *identical* text should be served from the exact cache.
CACHE_PROMPT = "What is the capital of France?"

# L1b semantic cache: paraphrased versions of CACHE_PROMPT.  After the
# exact-cache primer above has been stored, these semantically-similar
# prompts should trigger a cosine-similarity hit in the vector cache
# (layer 1b) instead of falling through to the SLM / Cloud layers.
SEMANTIC_CACHE_PROMPTS = [
    "What's the capital city of France?",
    "Tell me France's capital",
    "Which city is the capital of France?",
    "Capital of France?",
    "Can you name the capital of France?",
]

# L2 SLM: simple prompts the embedded model can handle locally.
SLM_PROMPTS = [
    "Hello",
    "Hi there",
    "What is 2+2?",
    "Say yes or no",
    "What color is the sky?",
]

# L3 Cloud: complex prompts that require the full external LLM.
CLOUD_PROMPTS = [
    "Write a Python script that scrapes Hacker News and stores the results in PostgreSQL with proper error handling and retry logic.",
    "Explain the differences between the CAP theorem and PACELC theorem with real-world examples from distributed databases.",
    "Design a microservices architecture for a real-time bidding platform handling 1 million requests per second.",
    "Compare and contrast the memory models of Rust, Go, and Java — include ownership, garbage collection, and escape analysis.",
    "Write a comprehensive technical blog post about implementing CRDT-based collaborative editing with operational transformation fallback.",
]


@dataclass
class LayerResult:
    """Aggregated results for a single layer benchmark."""

    name: str
    latencies: list[float] = field(default_factory=list)
    successes: int = 0
    failures: int = 0
    status_codes: dict[int, int] = field(default_factory=dict)
    responses: list[dict] = field(default_factory=list)

    @property
    def total(self) -> int:
        return self.successes + self.failures

    @property
    def p50(self) -> float:
        if not self.latencies:
            return 0.0
        s = sorted(self.latencies)
        return s[len(s) // 2]

    @property
    def p95(self) -> float:
        if not self.latencies:
            return 0.0
        s = sorted(self.latencies)
        idx = int(len(s) * 0.95)
        return s[min(idx, len(s) - 1)]

    @property
    def p99(self) -> float:
        if not self.latencies:
            return 0.0
        s = sorted(self.latencies)
        idx = int(len(s) * 0.99)
        return s[min(idx, len(s) - 1)]

    @property
    def mean(self) -> float:
        return statistics.mean(self.latencies) if self.latencies else 0.0

    @property
    def stdev(self) -> float:
        return statistics.stdev(self.latencies) if len(self.latencies) > 1 else 0.0

    @property
    def throughput(self) -> float:
        """Requests per second based on total wall-clock time."""
        if not self.latencies:
            return 0.0
        return self.total / sum(self.latencies) if sum(self.latencies) > 0 else 0.0


# ── Helpers ─────────────────────────────────────────────────────────


def send_request(url: str, prompt: str, api_key: str, timeout: float) -> tuple[float, int, dict | None]:
    """Send a single request and return (latency_seconds, status_code, json_body | None)."""
    headers = {"X-API-Key": api_key, "Content-Type": "application/json"}
    start = time.perf_counter()
    try:
        resp = requests.post(url, json={"prompt": prompt}, headers=headers, timeout=timeout)
        elapsed = time.perf_counter() - start
        try:
            body = resp.json()
        except ValueError:
            body = None
        return elapsed, resp.status_code, body
    except requests.exceptions.RequestException as exc:
        elapsed = time.perf_counter() - start
        return elapsed, 0, {"error": str(exc)}


def run_layer_benchmark(
    name: str,
    url: str,
    prompts: list[str],
    rounds: int,
    concurrency: int,
    api_key: str,
    timeout: float,
    warmup: int = 0,
) -> LayerResult:
    """Run `rounds` iterations of the given prompts (with optional warmup)."""
    result = LayerResult(name=name)

    # Build the full list of (round, prompt) tasks.
    tasks: list[tuple[int, str]] = []
    for r in range(rounds + warmup):
        for prompt in prompts:
            tasks.append((r, prompt))

    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = {
            pool.submit(send_request, url, prompt, api_key, timeout): (r, prompt)
            for r, prompt in tasks
        }
        for future in as_completed(futures):
            r, prompt = futures[future]
            latency, status, body = future.result()

            # Skip warmup rounds in statistics.
            if r < warmup:
                continue

            result.latencies.append(latency)
            result.status_codes[status] = result.status_codes.get(status, 0) + 1

            if 200 <= status < 300:
                result.successes += 1
            elif status == 502 and name == "L3_Cloud":
                # 502 means the SLM correctly routed to cloud but the
                # external LLM is unavailable — count as success for
                # routing purposes.
                result.successes += 1
            else:
                result.failures += 1

            if body:
                result.responses.append(body)

    return result


def print_result(r: LayerResult) -> None:
    """Pretty-print a single layer's benchmark results."""
    print(f"\n{'═' * 60}")
    print(f"  {r.name}")
    print(f"{'═' * 60}")
    print(f"  Requests   : {r.total}  (✓ {r.successes}  ✗ {r.failures})")
    print(f"  Status codes: {dict(sorted(r.status_codes.items()))}")
    print(f"  ────────────────────────────────────────")
    print(f"  Latency mean : {r.mean * 1000:8.1f} ms")
    print(f"  Latency p50  : {r.p50 * 1000:8.1f} ms")
    print(f"  Latency p95  : {r.p95 * 1000:8.1f} ms")
    print(f"  Latency p99  : {r.p99 * 1000:8.1f} ms")
    print(f"  Latency σ    : {r.stdev * 1000:8.1f} ms")
    print(f"  ────────────────────────────────────────")
    if r.latencies:
        wall = sum(r.latencies)
        print(f"  Throughput   : {r.throughput:8.2f} req/s  (serial sum)")
    # Show which layer actually handled the requests.
    layers_seen: dict[str, int] = {}
    for resp in r.responses:
        if isinstance(resp, dict) and "layer" in resp:
            layer_label = f"Layer {resp['layer']}"
            layers_seen[layer_label] = layers_seen.get(layer_label, 0) + 1
    if layers_seen:
        print(f"  Layers seen  : {dict(sorted(layers_seen.items()))}")


def print_comparison(results: list[LayerResult]) -> None:
    """Print a side-by-side comparison table."""
    print(f"\n\n{'━' * 72}")
    print("  LAYER COMPARISON")
    print(f"{'━' * 72}")
    header = f"  {'Layer':<20} {'Mean':>10} {'p50':>10} {'p95':>10} {'p99':>10} {'req/s':>8}"
    print(header)
    print(f"  {'─' * 68}")
    for r in results:
        print(
            f"  {r.name:<20} "
            f"{r.mean * 1000:>8.1f}ms "
            f"{r.p50 * 1000:>8.1f}ms "
            f"{r.p95 * 1000:>8.1f}ms "
            f"{r.p99 * 1000:>8.1f}ms "
            f"{r.throughput:>7.1f}"
        )

    # Speedup ratios (exact cache vs others).
    if results and results[0].mean > 0:
        baseline = results[0]
        baseline_mean = baseline.mean
        print(f"\n  Speedup vs {baseline.name}:")
        for r in results[1:]:
            if r.mean > 0:
                ratio = r.mean / baseline_mean
                print(f"    {r.name}: {ratio:.1f}× slower")
    print(f"{'━' * 72}\n")


# ── Main ────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Isartor AI Gateway — Layer Performance Benchmark",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Quick smoke test (2 rounds, sequential)
  python tests/perf_layers.py http://localhost:8000/api/chat

  # Full benchmark (10 rounds, 4 concurrent)
  python tests/perf_layers.py http://localhost:8000/api/chat --rounds 10 -c 4

  # Only test exact cache and semantic cache layers
  python tests/perf_layers.py http://localhost:8000/api/chat --layers exact-cache semantic-cache
""",
    )
    parser.add_argument(
        "url",
        nargs="?",
        default="http://localhost:8000/api/chat",
        help="Gateway chat endpoint (default: http://localhost:8000/api/chat)",
    )
    parser.add_argument(
        "--rounds", "-r",
        type=int,
        default=5,
        help="Number of measurement rounds per layer (default: 5)",
    )
    parser.add_argument(
        "--concurrency", "-c",
        type=int,
        default=1,
        help="Concurrent requests per layer (default: 1 = sequential)",
    )
    parser.add_argument(
        "--api-key",
        default="changeme",
        help="X-API-Key header value (default: changeme)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        help="Per-request timeout in seconds (default: 120)",
    )
    parser.add_argument(
        "--layers",
        nargs="+",
        default=["exact-cache", "semantic-cache", "slm", "cloud"],
        choices=["exact-cache", "semantic-cache", "slm", "cloud"],
        help="Which layers to benchmark (default: all four)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output results as JSON (machine-readable)",
    )

    args = parser.parse_args()
    url: str = args.url

    # ── Pre-flight check ──────────────────────────────────────────
    print(f"Isartor Layer Performance Benchmark")
    print(f"  Target  : {url}")
    print(f"  Rounds  : {args.rounds}")
    print(f"  Concur. : {args.concurrency}")
    print(f"  Layers  : {', '.join(args.layers)}")
    print(f"  Timeout : {args.timeout}s")

    # Verify connectivity.
    health_url = url.rsplit("/", 2)[0] + "/healthz"
    try:
        r = requests.get(health_url, timeout=5)
        if r.status_code == 200:
            print(f"  Health  : ✓ OK")
        else:
            print(f"  Health  : ⚠ status {r.status_code}")
    except requests.exceptions.RequestException:
        print(f"  Health  : ✗ unreachable ({health_url})")
        print(f"\n  Make sure the gateway is running, e.g.:")
        print(f"    docker compose -f docker/docker-compose.embedded.yml up --build")
        sys.exit(1)

    print()

    results: list[LayerResult] = []

    # ── Layer 1a: Exact Cache ────────────────────────────────────────
    if "exact-cache" in args.layers or "semantic-cache" in args.layers:
        # Both exact-cache and semantic-cache need the primer request.
        print("▶ Priming cache with initial request...")
        send_request(url, CACHE_PROMPT, args.api_key, args.timeout)
        # Small delay to let the cache index settle.
        time.sleep(0.5)

    if "exact-cache" in args.layers:
        print(f"▶ Benchmarking L1a Exact Cache ({args.rounds} rounds × 1 prompt)...")
        cache_result = run_layer_benchmark(
            name="L1a_ExactCache",
            url=url,
            prompts=[CACHE_PROMPT],
            rounds=args.rounds,
            concurrency=args.concurrency,
            api_key=args.api_key,
            timeout=args.timeout,
        )
        results.append(cache_result)
        print_result(cache_result)

    # ── Layer 1b: Semantic Cache ──────────────────────────────────
    if "semantic-cache" in args.layers:
        print(f"\n▶ Benchmarking L1b Semantic Cache ({args.rounds} rounds × {len(SEMANTIC_CACHE_PROMPTS)} prompts)...")
        semantic_result = run_layer_benchmark(
            name="L1b_SemanticCache",
            url=url,
            prompts=SEMANTIC_CACHE_PROMPTS,
            rounds=args.rounds,
            concurrency=args.concurrency,
            api_key=args.api_key,
            timeout=args.timeout,
        )
        results.append(semantic_result)
        print_result(semantic_result)

    # ── Layer 2: SLM ──────────────────────────────────────────────
    if "slm" in args.layers:
        print(f"\n▶ Benchmarking L2 SLM ({args.rounds} rounds × {len(SLM_PROMPTS)} prompts)...")
        slm_result = run_layer_benchmark(
            name="L2_SLM",
            url=url,
            prompts=SLM_PROMPTS,
            rounds=args.rounds,
            concurrency=args.concurrency,
            api_key=args.api_key,
            timeout=args.timeout,
        )
        results.append(slm_result)
        print_result(slm_result)

    # ── Layer 3: Cloud ────────────────────────────────────────────
    if "cloud" in args.layers:
        print(f"\n▶ Benchmarking L3 Cloud ({args.rounds} rounds × {len(CLOUD_PROMPTS)} prompts)...")
        cloud_result = run_layer_benchmark(
            name="L3_Cloud",
            url=url,
            prompts=CLOUD_PROMPTS,
            rounds=args.rounds,
            concurrency=args.concurrency,
            api_key=args.api_key,
            timeout=args.timeout,
        )
        results.append(cloud_result)
        print_result(cloud_result)

    # ── Summary ───────────────────────────────────────────────────
    if len(results) > 1:
        print_comparison(results)

    # ── JSON output ───────────────────────────────────────────────
    if args.json:
        json_out = []
        for r in results:
            json_out.append(
                {
                    "layer": r.name,
                    "total_requests": r.total,
                    "successes": r.successes,
                    "failures": r.failures,
                    "status_codes": r.status_codes,
                    "latency_mean_ms": round(r.mean * 1000, 2),
                    "latency_p50_ms": round(r.p50 * 1000, 2),
                    "latency_p95_ms": round(r.p95 * 1000, 2),
                    "latency_p99_ms": round(r.p99 * 1000, 2),
                    "latency_stdev_ms": round(r.stdev * 1000, 2),
                    "throughput_rps": round(r.throughput, 2),
                }
            )
        print(json.dumps(json_out, indent=2))


if __name__ == "__main__":
    main()
