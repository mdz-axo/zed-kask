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
COMPLEX_WORD_THRESHOLD = 9
SELECTION_BUDGET = 40
MATCH_SATURATION = 3.0
NAME_MATCH_SATURATION = 2.0
NAME_WEIGHT = 0.40
DESCRIPTION_WEIGHT = 0.35
INTENT_WEIGHT = 0.25
CODE_TOOL_NUDGE = 0.10
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


# ── The labelled eval set ────────────────────────────────────────────────────
#
# Each case: a request phrased as a user would phrase it, and the tools that
# genuinely serve it. `needed` is deliberately conservative -- only tools a
# competent operator would call for that request, verified to exist in the live
# surface. Phrasings are varied on purpose: some name the tool's own vocabulary,
# some describe the intent in plain language (the case keyword overlap handles
# worst, and the reason embeddings are under consideration).
EVAL_SET: list[dict] = [
    # ── Plain-language intent (no shared vocabulary with the description) ──
    {"id": "media-plain", "message": "make me a picture of a snowy mountain at sunset",
     "needed": ["generate_image"], "tag": "paraphrase"},
    {"id": "media-plain-2", "message": "turn this photo into a short video clip for me",
     "needed": ["image_to_video"], "tag": "paraphrase"},
    {"id": "speech-plain", "message": "read this paragraph out loud in a natural voice",
     "needed": ["generate_speech"], "tag": "paraphrase"},
    {"id": "transcribe-plain", "message": "what is being said in this audio recording",
     "needed": ["transcribe"], "tag": "paraphrase"},
    {"id": "code-plain", "message": "who calls this function and what breaks if i change it",
     "needed": ["codegraph_traverse", "codegraph_impact"], "tag": "paraphrase"},
    # Note: there is no `corpus_search`; semantic recall over stored material is
     # `curator_semantic_search`. The eval-set guard caught this mislabel.
    {"id": "corpus-plain", "message": "what do my saved documents say about interest rate policy",
     "needed": ["curator_semantic_search"], "tag": "paraphrase"},
    {"id": "kanban-plain", "message": "add a card to the board for fixing the parser bug",
     "needed": ["kanban_task_create"], "tag": "paraphrase"},
    {"id": "forecast-plain", "message": "how likely is it that this event happens before december",
     "needed": ["market_lookup"], "tag": "paraphrase"},

    # ── Tool-vocabulary phrasing (keyword overlap should do well) ──
    {"id": "web-search", "message": "search the web for recent papers on retrieval augmented generation",
     "needed": ["web_search"], "tag": "vocabulary"},
    {"id": "web-extract", "message": "extract the readable content from this article url https://example.com/x",
     "needed": ["web_extract"], "tag": "vocabulary"},
    {"id": "codegraph-query", "message": "query the codegraph for the symbol that parses configuration",
     "needed": ["codegraph_query"], "tag": "vocabulary"},
    {"id": "gallery", "message": "search the media gallery for the mountain images i generated",
     "needed": ["gallery_search"], "tag": "vocabulary"},
    {"id": "portfolio", "message": "show me a snapshot of my portfolio positions and returns",
     "needed": ["portfolio_snapshot", "portfolio_returns"], "tag": "vocabulary"},
    {"id": "training", "message": "assemble the training dataset and submit a fine tuning run",
     "needed": ["training_assemble_dataset", "training_submit"], "tag": "vocabulary"},
    {"id": "scenario", "message": "build a scenario matrix from the current prediction markets",
     "needed": ["scenario_from_markets"], "tag": "vocabulary"},
    {"id": "condenser", "message": "condense this long thread into a short summary for me",
     "needed": ["condenser_thread_summary"], "tag": "vocabulary"},
    {"id": "curator", "message": "show me the outstanding curator escalations for review",
     "needed": ["curator_escalations"], "tag": "vocabulary"},
    {"id": "ocr", "message": "run ocr over this scanned pdf and convert it to text",
     "needed": ["corpus_ocr", "corpus_convert"], "tag": "vocabulary"},

    # ── Multi-tool / compound requests ──
    {"id": "compound-media", "message": "generate an image of a mountain then upscale it and add it to the gallery",
     "needed": ["generate_image", "upscale_image"], "tag": "compound"},
    {"id": "compound-research", "message": "search the web for the latest earnings coverage then extract the top article",
     "needed": ["web_search", "web_extract"], "tag": "compound"},
    {"id": "compound-code", "message": "find the dead code in this project and analyze the complexity of the worst offenders",
     "needed": ["codegraph_analysis"], "tag": "compound"},

    # ── Conversationally padded (the dilution case) ──
    {"id": "padded-media", "message": "i was wondering earlier today whether you might be able to help me out with something here since i would really like you to make me a picture of a snowy mountain",
     "needed": ["generate_image"], "tag": "padded"},
    {"id": "padded-web", "message": "when you get a chance and if it is not too much trouble could you please go and search the web for recent commentary on this topic",
     "needed": ["web_search"], "tag": "padded"},

    # ── Should fail open (too short / no actionable signal) ──
    {"id": "greeting", "message": "hello", "needed": [], "tag": "fail-open"},
    {"id": "terse", "message": "fix this", "needed": [], "tag": "fail-open"},
    {"id": "vague", "message": "what does this do", "needed": [], "tag": "fail-open"},
]


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
            ("NO_CONFIDENCE_FLOOR", NO_CONFIDENCE_FLOOR),
        ):
            print(f"  {name} = {value}")
        return 0

    tools = load_tools()
    names = {t["name"] for t in tools}
    total = len(tools)

    # Fail loudly if the eval set references a tool that no longer exists --
    # a silently-stale label would inflate or deflate recall.
    missing = sorted(
        {n for case in EVAL_SET for n in case["needed"] if n not in names}
    )
    if missing:
        print(f"ERROR: eval set references non-existent tools: {missing}")
        return 1

    results = []
    for case in EVAL_SET:
        retained, activated = route(case["message"], tools)
        retained_set = set(retained)
        needed = case["needed"]
        found = [n for n in needed if n in retained_set]
        recall = 1.0 if not needed else len(found) / len(needed)
        results.append(
            {
                "id": case["id"],
                "tag": case["tag"],
                "words": len(case["message"].split()),
                "activated": activated,
                "kept": len(retained),
                "recall": recall,
                "missed": [n for n in needed if n not in retained_set],
            }
        )

    scored = [r for r in results if r["tag"] != "fail-open"]
    mean_recall = sum(r["recall"] for r in scored) / max(len(scored), 1)
    perfect = sum(1 for r in scored if r["recall"] == 1.0)
    mean_kept = sum(r["kept"] for r in scored) / max(len(scored), 1)
    fail_open_ok = all(
        r["kept"] == total for r in results if r["tag"] == "fail-open"
    )

    if args.json:
        print(json.dumps({"total_tools": total, "summary": {
            "mean_recall": mean_recall, "perfect_recall_cases": perfect,
            "scored_cases": len(scored), "mean_kept": mean_kept,
            "fail_open_correct": fail_open_ok}, "cases": results}, indent=2))
        return 0

    print(f"\nTool surface: {total} MCP tools | eval cases: {len(EVAL_SET)}")
    print("=" * 78)
    if args.verbose:
        print(f"{'case':<18}{'tag':<12}{'w':>3}{'kept':>6}{'recall':>8}  missed")
        for r in results:
            miss = ",".join(r["missed"]) or "-"
            print(
                f"{r['id']:<18}{r['tag']:<12}{r['words']:>3}{r['kept']:>6}"
                f"{r['recall']:>8.2f}  {miss}"
            )
        print()

    by_tag: dict[str, list[dict]] = {}
    for r in scored:
        by_tag.setdefault(r["tag"], []).append(r)
    print(f"{'tag':<14}{'cases':>6}{'recall':>9}{'mean kept':>11}")
    for tag, rows in sorted(by_tag.items()):
        rc = sum(x["recall"] for x in rows) / len(rows)
        kp = sum(x["kept"] for x in rows) / len(rows)
        print(f"{tag:<14}{len(rows):>6}{rc:>9.2f}{kp:>11.1f}")

    print("-" * 78)
    print(f"{'OVERALL':<14}{len(scored):>6}{mean_recall:>9.2f}{mean_kept:>11.1f}")
    print()
    print(f"  Perfect-recall cases : {perfect}/{len(scored)}")
    print(f"  Mean retained        : {mean_kept:.1f} of {total} "
          f"({100.0 * mean_kept / total:.0f}%)")
    print(f"  Fail-open correct    : {'yes' if fail_open_ok else 'NO'}")
    print()
    print("Recall is the metric that matters: a dropped tool costs a failed turn,")
    print("a spare one costs ~45 tokens. Mean-kept is the token side of the trade.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
