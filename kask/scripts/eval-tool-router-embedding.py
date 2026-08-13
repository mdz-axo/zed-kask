#!/usr/bin/env python3
"""Experiment: does embedding similarity beat keyword scoring for tool routing?

Compares three rankers against the same labelled eval set the keyword router is
measured on, so the comparison is apples-to-apples:

  keyword   - the shipped LazyToolRouter logic (mirrored in eval-tool-router.py)
  embedding - cosine similarity between the request and each tool's
              "name + description", via a local Ollama embedding model
  hybrid    - max of the two normalised scores

The question is NOT "is embedding similarity better at ranking" -- it is whether
it beats the *current shipped behaviour*, which already achieves recall 1.000 by
failing open on hard cases. Embeddings only pay for themselves if they hold that
recall while retaining substantially fewer tools.

Requires Ollama with an embedding model pulled:
  ollama pull qwen3-embedding:0.6b

Usage:
  python3 kask/scripts/eval-tool-router-embedding.py                # all rankers
  python3 kask/scripts/eval-tool-router-embedding.py --budget 40
  python3 kask/scripts/eval-tool-router-embedding.py --model mxbai-embed-large:335m
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
OLLAMA = "http://localhost:11434/api/embed"
CACHE = Path("/tmp/tool-router-embeddings")


def load_keyword_module():
    """Reuse the keyword harness so both rankers share tools and eval cases."""
    spec = importlib.util.spec_from_file_location("kw", HERE / "eval-tool-router.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def embed_batch(texts: list[str], model: str, timeout: int = 300) -> list[list[float]]:
    payload = json.dumps({"model": model, "input": texts}).encode()
    request = urllib.request.Request(
        OLLAMA, data=payload, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        data = json.loads(response.read())
    vectors = data.get("embeddings")
    if vectors is None:
        raise RuntimeError(f"no embeddings in response: {list(data.keys())}")
    return vectors


def embed_cached(texts: list[str], model: str, label: str) -> list[list[float]]:
    """Embed with an on-disk cache. Tool descriptions never change between runs,
    so re-embedding them each time would dominate the measurement."""
    CACHE.mkdir(exist_ok=True)
    key = CACHE / f"{label}-{model.replace(':', '_').replace('/', '_')}.json"
    if key.exists():
        cached = json.loads(key.read_text())
        if cached.get("texts") == texts:
            return cached["vectors"]
    vectors: list[list[float]] = []
    # Batch to keep request bodies reasonable.
    for start in range(0, len(texts), 32):
        chunk = texts[start : start + 32]
        vectors.extend(embed_batch(chunk, model))
        print(f"    embedded {min(start + 32, len(texts))}/{len(texts)}", end="\r")
    print(" " * 40, end="\r")
    key.write_text(json.dumps({"texts": texts, "vectors": vectors}))
    return vectors


def normalise(vector: list[float]) -> list[float]:
    norm = math.sqrt(sum(component * component for component in vector))
    return [component / norm for component in vector] if norm else vector


def cosine(a: list[float], b: list[float]) -> float:
    return sum(x * y for x, y in zip(a, b))


def summarise(rows: list[dict], total: int) -> dict:
    graded = [r for r in rows if r["tag"] != "fail-open"]
    opens = [r for r in rows if r["tag"] == "fail-open"]
    return {
        "cases": len(graded),
        "recall": sum(r["recall"] for r in graded) / max(len(graded), 1),
        "perfect": sum(1 for r in graded if r["recall"] == 1.0),
        "mean_kept": sum(r["kept"] for r in graded) / max(len(graded), 1),
        "fail_open_correct": all(r["kept"] == total for r in opens),
        "fail_open_cases": len(opens),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="qwen3-embedding:0.6b")
    parser.add_argument("--budget", type=int, default=40)
    parser.add_argument(
        "--sim-gate",
        type=float,
        default=None,
        help="minimum top cosine required to prune; below it, fail open",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    kw = load_keyword_module()
    tools = kw.load_tools()
    cases = kw.load_eval_set()
    total = len(tools)
    names = {t["name"] for t in tools}
    missing = sorted({n for c in cases for n in c["needed"] if n not in names})
    if missing:
        print(f"ERROR: eval set references non-existent tools: {missing}")
        return 1

    print(f"Tool surface: {total} | cases: {len(cases)} | model: {args.model}")

    # Embed the tool surface once (cached), then the queries.
    tool_texts = [f"{t['name']}: {t['description']}" for t in tools]
    print("  embedding tool surface...")
    try:
        tool_vectors = [normalise(v) for v in embed_cached(tool_texts, args.model, "tools")]
    except (urllib.error.URLError, RuntimeError, TimeoutError) as error:
        print(f"ERROR: embedding failed ({error}). Is Ollama running and the model pulled?")
        return 1

    query_texts = [c["message"] for c in cases]
    print("  embedding queries...")
    started = time.monotonic()
    query_vectors = [normalise(v) for v in embed_cached(query_texts, args.model, "queries")]
    embed_seconds = time.monotonic() - started

    results: dict[str, list[dict]] = {"keyword": [], "embedding": [], "hybrid": []}

    for case, query_vector in zip(cases, query_vectors):
        needed = case["needed"]

        # ── keyword: the shipped behaviour, verbatim ──
        retained, _ = kw.route(case["message"], tools)
        kept = set(retained)
        results["keyword"].append(
            {
                "id": case["id"],
                "tag": case["tag"],
                "split": case["split"],
                "kept": len(retained),
                "recall": 1.0
                if not needed
                else sum(1 for n in needed if n in kept) / len(needed),
                "missed": [n for n in needed if n not in kept],
            }
        )

        # ── embedding: rank by cosine, take budget ──
        sims = sorted(
            ((cosine(query_vector, tv), tools[i]["name"]) for i, tv in enumerate(tool_vectors)),
            key=lambda pair: -pair[0],
        )
        top_sim = sims[0][0]
        if args.sim_gate is not None and top_sim < args.sim_gate:
            emb_names = [t["name"] for t in tools]  # fail open
        else:
            emb_names = [n for _, n in sims[: args.budget]]
        emb_kept = set(emb_names)
        results["embedding"].append(
            {
                "id": case["id"],
                "tag": case["tag"],
                "split": case["split"],
                "kept": len(emb_names),
                "top_sim": top_sim,
                "recall": 1.0
                if not needed
                else sum(1 for n in needed if n in emb_kept) / len(needed),
                "missed": [n for n in needed if n not in emb_kept],
            }
        )

        # ── hybrid: union of keyword selection and embedding top-N ──
        # Union rather than max-of-scores: the two rankers fail on disjoint
        # cases, so a union preserves both recalls at the cost of set size.
        if len(retained) == total:
            hybrid_names = emb_names  # keyword failed open; trust embedding
        else:
            hybrid_names = list(set(retained) | set(emb_names[: args.budget // 2]))
        hybrid_kept = set(hybrid_names)
        results["hybrid"].append(
            {
                "id": case["id"],
                "tag": case["tag"],
                "split": case["split"],
                "kept": len(hybrid_names),
                "recall": 1.0
                if not needed
                else sum(1 for n in needed if n in hybrid_kept) / len(needed),
                "missed": [n for n in needed if n not in hybrid_kept],
            }
        )

    if args.json:
        print(json.dumps({r: summarise(v, total) for r, v in results.items()}, indent=2))
        return 0

    print(f"  query embedding wall time: {embed_seconds:.1f}s "
          f"({1000 * embed_seconds / max(len(cases), 1):.0f}ms/query amortised)\n")

    print("=" * 82)
    print(f"{'ranker':<11}{'split':<10}{'cases':>6}{'recall':>9}{'kept':>8}{'perfect':>10}")
    print("-" * 82)
    for ranker, rows in results.items():
        for split in ("dev", "holdout", "ALL"):
            subset = rows if split == "ALL" else [r for r in rows if r["split"] == split]
            s = summarise(subset, total)
            print(
                f"{ranker if split == 'dev' else '':<11}{split:<10}{s['cases']:>6}"
                f"{s['recall']:>9.3f}{s['mean_kept']:>8.1f}"
                f"{s['perfect']:>6}/{s['cases']}"
            )
        print("-" * 82)

    print("\nPer-phrasing recall (holdout only, the honest split):")
    tags = sorted({r["tag"] for r in results["keyword"] if r["tag"] != "fail-open"})
    print(f"{'phrasing':<14}" + "".join(f"{r:>22}" for r in results))
    for tag in tags:
        line = f"{tag:<14}"
        for rows in results.values():
            subset = [r for r in rows if r["tag"] == tag and r["split"] == "holdout"]
            if subset:
                rc = sum(r["recall"] for r in subset) / len(subset)
                kp = sum(r["kept"] for r in subset) / len(subset)
                line += f"{rc:>13.3f} @{kp:>6.0f}"
            else:
                line += f"{'-':>22}"
        print(line)

    print("\nFail-open safety (must retain all tools on vague messages):")
    for ranker, rows in results.items():
        s = summarise(rows, total)
        print(f"  {ranker:<11}{'yes' if s['fail_open_correct'] else 'NO'} "
              f"({s['fail_open_cases']} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
