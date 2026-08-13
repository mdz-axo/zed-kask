#!/usr/bin/env python3
"""Baseline the agent tool router against a labelled request -> tools eval set.

Answers one question: for a realistic request, does the router keep the tools the
request actually needs? Reports recall (the metric that matters -- a dropped tool
costs a failed turn) alongside the retained-set size (the token cost), so the
tradeoff is visible rather than asserted.

The router logic is reimplemented here in Python, mirroring
`crates/agent/src/tool_router.rs`. That is a deliberate tradeoff: it makes the
eval runnable in seconds against the live 252-tool surface without a Rust harness,
at the cost of needing to stay in sync. `--check-parity` prints the constants this
script assumes so drift is visible; the Rust tests remain authoritative for
behaviour.

Tool descriptions are extracted from the real `#[tool(description = ...)]`
attributes in `kask/mcp-servers/`, so the eval runs against what actually ships.

Usage:
  python3 kask/scripts/eval-tool-router.py            # baseline current router
  python3 kask/scripts/eval-tool-router.py --verbose  # per-case detail
  python3 kask/scripts/eval-tool-router.py --json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# ── Router constants, mirrored from crates/agent/src/tool_router.rs ──────────
THRESHOLD = 0.30
COMPLEX_WORD_THRESHOLD = 6
SELECTION_BUDGET = 40
MATCH_SATURATION = 3.0
NAME_MATCH_SATURATION = 2.0
NAME_WEIGHT = 0.40
DESCRIPTION_WEIGHT = 0.35
INTENT_WEIGHT = 0.25
CODE_TOOL_NUDGE = 0.10
CONFIDENCE_GATE = 0.50
NO_CONFIDENCE_FLOOR = 1

STOPWORDS = {
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was",
    "one", "our", "out", "day", "get", "has", "him", "his", "how", "man", "new",
    "now", "old", "see", "two", "way", "who", "boy", "did", "its", "let", "put",
    "say", "she", "too", "use", "that", "this", "with", "have", "from", "they",
    "will", "would", "there", "their", "what", "about", "which", "when", "make",
    "like", "time", "just", "some", "could", "them", "than", "then", "into",
    "your", "want", "please", "need",
}

TOOL_NAME_SIGNALS = [
    "grep", "read file", "read_file", "edit file", "edit_file", "write file",
    "write_file", "terminal", "fetch", "web search", "web_search", "find path",
    "find_path", "list directory", "list_directory", "diagnostics",
    "find references", "find_references", "code action", "go to definition",
    "rename", "spawn agent", "spawn_agent", "create thread", "create_thread",
]

COMPLEX_SIGNALS = [
    "plan", "break down", "decompose", "multiple steps", "subagent", "delegate",
    "parallel", "coordinate", "orchestrate",
]

CODE_SIGNALS = {
    "edit", "write", "read", "fix", "refactor", "search", "grep", "find",
    "delete", "create", "move", "rename", "diagnostic", "debug", "test",
    "build", "compile",
}

INTENT_KEYWORDS = {"url", "fetch", "web", "terminal", "shell", "command", "search"}

CODE_TOOL_KEYWORDS = [
    "file", "edit", "write", "read", "grep", "search", "directory", "path",
    "diagnostic", "definition", "reference", "rename", "code action", "symbol",
]


def tokenize(text: str) -> set[str]:
    return {t for t in re.split(r"[^a-z0-9]+", text.lower()) if t}


def extract_context_keywords(message: str) -> set[str]:
    keywords = set()
    for word in message.split():
        cleaned = re.sub(r"^[^a-z0-9_-]+|[^a-z0-9_-]+$", "", word.lower())
        if len(cleaned) > 2 and cleaned not in STOPWORDS:
            keywords.add(cleaned)
    lower = message.lower()
    if "http://" in lower or "https://" in lower:
        keywords |= {"url", "fetch", "web"}
    if any(s in lower for s in ("terminal", "run ", "execute", "command", "shell")):
        keywords |= {"terminal", "shell", "command"}
    if any(s in lower for s in ("search", "find ", "grep")):
        keywords.add("search")
    return keywords


def should_activate(message: str, has_code_file: bool) -> bool:
    lower = message.lower()
    if any(signal in lower for signal in TOOL_NAME_SIGNALS):
        return True
    if len(message.split()) >= COMPLEX_WORD_THRESHOLD:
        return True
    if any(signal in lower for signal in COMPLEX_SIGNALS):
        return True
    if has_code_file and any(signal in lower for signal in CODE_SIGNALS):
        return True
    return False


def score_tool(
    name: str, description: str, keywords: set[str], has_code_file: bool
) -> float:
    description_lower = description.lower()
    terms = tokenize(description_lower)
    name_terms = tokenize(name)

    matched = sum(1 for kw in keywords if kw.lower() in terms)
    match_evidence = min(matched / MATCH_SATURATION, 1.0)

    name_matched = sum(1 for kw in keywords if kw.lower() in name_terms)
    name_evidence = min(name_matched / NAME_MATCH_SATURATION, 1.0)

    intent_matched = sum(
        1 for kw in keywords if kw in INTENT_KEYWORDS and kw.lower() in terms
    )
    score = (
        NAME_WEIGHT * name_evidence
        + DESCRIPTION_WEIGHT * match_evidence
        + INTENT_WEIGHT * min(intent_matched, 1)
    )

    # Additive nudge, gated on an open code file only -- not on generic verbs.
    if has_code_file:
        boosted = any(
            (kw in description_lower) if " " in kw else (kw in terms)
            for kw in CODE_TOOL_KEYWORDS
        )
        if boosted:
            score += CODE_TOOL_NUDGE
    return min(score, 1.0)


def route(message: str, tools: list[dict], has_code_file: bool = False):
    """Return (retained_names, activated). Mirrors apply_router + select_tools."""
    if not should_activate(message, has_code_file):
        return [t["name"] for t in tools], False

    keywords = extract_context_keywords(message)
    scored = sorted(
        (
            (score_tool(t["name"], t["description"], keywords, has_code_file), t)
            for t in tools
        ),
        key=lambda pair: (-pair[0], pair[1]["name"]),
    )
    # Confidence gate: only prune when the best match is strong enough that the
    # ranking can be trusted. A small confident selection is worse than none.
    top_score = scored[0][0] if scored else 0.0
    if top_score < CONFIDENCE_GATE:
        return [t["name"] for t in tools], True

    selected = [t["name"] for score, t in scored[:SELECTION_BUDGET] if score >= THRESHOLD]

    # Empty selection is scorer failure, not a narrowing -> fail open.
    if len(selected) < NO_CONFIDENCE_FLOOR:
        return [t["name"] for t in tools], True
    return selected, True


def load_tools() -> list[dict]:
    tools: list[dict] = []
    servers = REPO / "kask/mcp-servers"
    pattern = re.compile(
        r'#\[tool\s*\(\s*description\s*=\s*"((?:[^"\\]|\\.)*)"\s*\)\]\s*'
        r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)",
        re.S,
    )
    for server in sorted(p for p in servers.iterdir() if p.is_dir()):
        for src in server.rglob("*.rs"):
            if "/tests/" in str(src):
                continue
            text = src.read_text(encoding="utf-8", errors="replace")
            for match in pattern.finditer(text):
                tools.append(
                    {
                        "server": server.name,
                        "name": match.group(2),
                        "description": " ".join(match.group(1).split()),
                    }
                )
    return tools


EVAL_SET_PATH = Path(__file__).resolve().parent / "tool-router-eval-set.json"


def load_eval_set() -> list[dict]:
    """Load the labelled cases from JSON, normalising the `phrasing` key to `tag`."""
    data = json.loads(EVAL_SET_PATH.read_text(encoding="utf-8"))
    cases = []
    for case in data["cases"]:
        cases.append(
            {
                "id": case["id"],
                "message": case["message"],
                "needed": case["needed"],
                "tag": case["phrasing"],
                "split": case.get("split", "dev"),
            }
        )
    return cases




def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--check-parity", action="store_true")
    args = parser.parse_args()

    if args.check_parity:
        print("Constants this script mirrors from crates/agent/src/tool_router.rs:")
        for name, value in (
            ("THRESHOLD", THRESHOLD),
            ("COMPLEX_WORD_THRESHOLD", COMPLEX_WORD_THRESHOLD),
            ("SELECTION_BUDGET", SELECTION_BUDGET),
            ("MATCH_SATURATION", MATCH_SATURATION),
            ("NAME_MATCH_SATURATION", NAME_MATCH_SATURATION),
            ("NAME_WEIGHT", NAME_WEIGHT),
            ("DESCRIPTION_WEIGHT", DESCRIPTION_WEIGHT),
            ("INTENT_WEIGHT", INTENT_WEIGHT),
            ("CODE_TOOL_NUDGE", CODE_TOOL_NUDGE),
            ("CONFIDENCE_GATE", CONFIDENCE_GATE),
            ("NO_CONFIDENCE_FLOOR", NO_CONFIDENCE_FLOOR),
        ):
            print(f"  {name} = {value}")
        return 0

    tools = load_tools()
    names = {t["name"] for t in tools}
    total = len(tools)
    eval_set = load_eval_set()

    # Fail loudly if the eval set references a tool that no longer exists --
    # a silently-stale label would inflate or deflate recall.
    missing = sorted(
        {n for case in eval_set for n in case["needed"] if n not in names}
    )
    if missing:
        print(f"ERROR: eval set references non-existent tools: {missing}")
        return 1

    results = []
    for case in eval_set:
        retained, activated = route(case["message"], tools)
        retained_set = set(retained)
        needed = case["needed"]
        found = [n for n in needed if n in retained_set]
        recall = 1.0 if not needed else len(found) / len(needed)
        results.append(
            {
                "id": case["id"],
                "tag": case["tag"],
                "split": case["split"],
                "words": len(case["message"].split()),
                "activated": activated,
                "kept": len(retained),
                "recall": recall,
                "missed": [n for n in needed if n not in retained_set],
            }
        )

    def summarise(rows: list[dict]) -> dict:
        graded = [r for r in rows if r["tag"] != "fail-open"]
        opens = [r for r in rows if r["tag"] == "fail-open"]
        return {
            "cases": len(graded),
            "recall": sum(r["recall"] for r in graded) / max(len(graded), 1),
            "perfect": sum(1 for r in graded if r["recall"] == 1.0),
            "mean_kept": sum(r["kept"] for r in graded) / max(len(graded), 1),
            "fail_open_cases": len(opens),
            "fail_open_correct": all(r["kept"] == total for r in opens),
        }

    overall = summarise(results)
    dev = summarise([r for r in results if r["split"] == "dev"])
    holdout = summarise([r for r in results if r["split"] == "holdout"])

    if args.json:
        print(json.dumps({"total_tools": total, "overall": overall, "dev": dev,
                          "holdout": holdout, "cases": results}, indent=2))
        return 0

    print(f"\nTool surface: {total} MCP tools | eval cases: {len(eval_set)}")
    print("=" * 78)
    if args.verbose:
        print(f"{'case':<7}{'split':<9}{'tag':<13}{'w':>3}{'kept':>6}{'recall':>8}  missed")
        for r in results:
            if r["recall"] == 1.0 and not args.json and r["tag"] != "fail-open":
                continue  # only show imperfect cases; full detail via --json
            miss = ",".join(r["missed"]) or "-"
            print(
                f"{r['id']:<7}{r['split']:<9}{r['tag']:<13}{r['words']:>3}"
                f"{r['kept']:>6}{r['recall']:>8.2f}  {miss}"
            )
        print("  (perfect-recall cases omitted; use --json for all)\n")

    graded = [r for r in results if r["tag"] != "fail-open"]
    by_tag: dict[str, list[dict]] = {}
    for r in graded:
        by_tag.setdefault(r["tag"], []).append(r)
    print(f"{'phrasing':<14}{'cases':>6}{'recall':>9}{'mean kept':>11}")
    for tag, rows in sorted(by_tag.items(), key=lambda kv: -len(kv[1])):
        rc = sum(x["recall"] for x in rows) / len(rows)
        kp = sum(x["kept"] for x in rows) / len(rows)
        print(f"{tag:<14}{len(rows):>6}{rc:>9.2f}{kp:>11.1f}")

    print("-" * 78)
    print(f"{'split':<14}{'cases':>6}{'recall':>9}{'mean kept':>11}{'perfect':>10}")
    for label, s in (("dev (tuned)", dev), ("holdout", holdout), ("ALL", overall)):
        print(
            f"{label:<14}{s['cases']:>6}{s['recall']:>9.2f}"
            f"{s['mean_kept']:>11.1f}{s['perfect']:>6}/{s['cases']}"
        )
    print()
    print(f"  Mean retained (all)  : {overall['mean_kept']:.1f} of {total} "
          f"({100.0 * overall['mean_kept'] / total:.0f}%)")
    print(f"  Fail-open correct    : "
          f"{'yes' if overall['fail_open_correct'] else 'NO'} "
          f"({overall['fail_open_cases']} cases)")
    gap = dev["recall"] - holdout["recall"]
    print(f"  Dev - holdout gap    : {gap:+.2f} "
          f"({'overfitting risk' if gap > 0.10 else 'generalises'})")
    print()
    print("Recall is the metric that matters: a dropped tool costs a failed turn,")
    print("a spare one costs ~45 tokens. Mean-kept is the token side of the trade.")
    print("The holdout split was written without consulting scores; treat it as the")
    print("honest number and never tune against it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
