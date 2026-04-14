#!/usr/bin/env python3
"""Merge all training-pair sources into one clean dataset + validate.

Inputs:
  data/pairs/train.jsonl         — synthesized pairs from synthesize_pairs.py
  data/seeds/architecture_seeds.jsonl — hand-written seed pairs (promoted verbatim)
  data/seeds/chris_memory_facts.jsonl — claude memory facts (promoted as Q/A)
  data/seeds/chris_voice_raw.jsonl    — real chris assistant replies (optional)

Output:
  data/pairs/train.merged.jsonl  — final dataset, rubric-validated
  data/pairs/train.stats.json    — counts by kind/rule
  data/pairs/train.dropped.jsonl — rows that failed validation (for debugging)

Validation is heuristic — we look for the presence of rubric markers in
the assistant response (e.g. `: Sendable`, `@Observable`, `@Entry`,
`MARK: - `) and drop rows that don't match the expected pattern for
their kind. This is coarse on purpose — we'd rather drop a good row
than train on a bad one.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
PAIRS_DIR = ROOT / "data/pairs"
SEEDS_DIR = ROOT / "data/seeds"

SYN_PATH = PAIRS_DIR / "train.jsonl"
SEED_PATH = SEEDS_DIR / "architecture_seeds.jsonl"
FACTS_PATH = SEEDS_DIR / "chris_memory_facts.jsonl"
CHRIS_RAW = SEEDS_DIR / "chris_voice_raw.jsonl"

OUT_PATH = PAIRS_DIR / "train.merged.jsonl"
STATS_PATH = PAIRS_DIR / "train.stats.json"
DROP_PATH = PAIRS_DIR / "train.dropped.jsonl"

SYSTEM_PROMPT = (
    "You are flacoAi, a local-only AI brain for Chris Roura (Roura.io). "
    "You are expert in Swift 6, SwiftUI, strict concurrency, and Chris's "
    "protocol-oriented architecture. You follow his rubric exactly: init "
    "DI for VM-backed views, @Entry + environment as fallback, actor "
    "wrappers for I/O, manager facades with .live/.mock factories, "
    "MARK-sectioned files, dedicated error enums. Never use "
    "@EnvironmentObject, singletons, @StateObject with parameterless "
    "init, or completion-handler APIs."
)


# Rubric markers we expect depending on which rule a row is training.
RULE_MARKERS: dict[str, list[re.Pattern[str]]] = {
    "R1": [re.compile(r"public\s+protocol\s+\w+\s*:\s*Sendable", re.M),
           re.compile(r"typealias\s+\w+\s*=\s*\w+(\s*&\s*\w+)+", re.M)],
    "R2": [re.compile(r"@Observable\s+public\s+final\s+class", re.M),
           re.compile(r"static\s+(func\s+live|var\s+mock)", re.M)],
    "R3": [re.compile(r"public\s+actor\s+\w+", re.M)],
    "R4": [re.compile(r"@Entry\s+var\s+\w+", re.M),
           re.compile(r"_viewModel\s*=\s*State\(wrappedValue:", re.M),
           re.compile(r"@Environment\(\\\.\w+\)", re.M),
           re.compile(r"\.environment\(\\\.\w+,", re.M)],
    "R5": [re.compile(r"public\s+final\s+class\s+Mock\w+", re.M),
           re.compile(r"public\s+private\(set\)", re.M)],
    "R9": [re.compile(r"public\s+enum\s+\w+Error\s*:\s*Error", re.M)],
}


def looks_like_swift(text: str) -> bool:
    return bool(re.search(r"\b(public|private|fileprivate|final|struct|class|extension|protocol|actor|enum|func)\s", text))


def validates(content: str, rule: str | None) -> bool:
    """Heuristic rubric validation. Missing rule = skip validation."""
    if not rule:
        return True
    patterns = RULE_MARKERS.get(rule)
    if not patterns:
        return True
    return any(p.search(content) for p in patterns)


def iter_jsonl(path: Path):
    if not path.exists():
        return
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue


def row_from_seed(seed: dict[str, Any]) -> dict[str, Any]:
    return {
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": seed["prompt"]},
            {"role": "assistant", "content": seed["ideal_response"]},
        ],
        "meta": {"kind": seed.get("kind", "architecture"),
                 "rubric_rule": seed.get("rule"),
                 "source": "hand_seed"},
    }


def row_from_fact(fact: dict[str, Any], idx: int) -> dict[str, Any]:
    # Turn a memory fact into a Q/A about Chris/his world so the model
    # internalizes personal context it can recall without the retrieval layer.
    body = fact["content"]
    q = f"Tell me what you know about this topic from your memory of Chris: {fact.get('source_file','')[:40]}"
    return {
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": q},
            {"role": "assistant", "content": body},
        ],
        "meta": {"kind": "chris_fact", "rubric_rule": None, "source": "claude_memory"},
    }


def main() -> None:
    kept: list[dict[str, Any]] = []
    dropped: list[dict[str, Any]] = []
    stats: dict[str, int] = {}

    # 1. hand-written seeds (always kept, promoted verbatim)
    for seed in iter_jsonl(SEED_PATH):
        row = row_from_seed(seed)
        kept.append(row)
        key = f"seed:{row['meta'].get('rubric_rule','?')}"
        stats[key] = stats.get(key, 0) + 1

    # 2. synthesized pairs (validated)
    for row in iter_jsonl(SYN_PATH):
        if "messages" not in row or len(row["messages"]) < 3:
            dropped.append(row)
            continue
        assistant = row["messages"][2].get("content", "")
        rule = (row.get("meta") or {}).get("rubric_rule")
        if rule in RULE_MARKERS and not validates(assistant, rule):
            dropped.append(row)
            continue
        # For code-shaped rules, require the response to actually contain Swift
        if rule in ("R1", "R2", "R3", "R4", "R5", "R9") and not looks_like_swift(assistant):
            dropped.append(row)
            continue
        kept.append(row)
        key = f"syn:{rule or 'misc'}"
        stats[key] = stats.get(key, 0) + 1

    # 3. memory facts
    for idx, fact in enumerate(iter_jsonl(FACTS_PATH)):
        if not fact.get("content"):
            continue
        kept.append(row_from_fact(fact, idx))
        stats["memory_fact"] = stats.get("memory_fact", 0) + 1

    with OUT_PATH.open("w", encoding="utf-8") as fh:
        for row in kept:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")
    with DROP_PATH.open("w", encoding="utf-8") as fh:
        for row in dropped:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")

    stats["_total_kept"] = len(kept)
    stats["_total_dropped"] = len(dropped)
    STATS_PATH.write_text(json.dumps(stats, indent=2), encoding="utf-8")

    print(f"kept    {len(kept):>5}  → {OUT_PATH}")
    print(f"dropped {len(dropped):>5}  → {DROP_PATH}")
    print("by bucket:")
    for k, v in sorted(stats.items()):
        if not k.startswith("_"):
            print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
