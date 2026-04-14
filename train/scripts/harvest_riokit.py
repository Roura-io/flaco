#!/usr/bin/env python3
"""Harvest positive examples from RIOExperimentationKit.

RIOKit is the single cleanest encoding of Chris's architecture pattern,
so we treat it as the primary source of truth. Every .swift file is
copied into data/raw/riokit/ and each type is also emitted as a
standalone "exemplar" file tagged with the rubric rule it demonstrates.

Downstream, synthesize_pairs.py uses these exemplars as few-shot
context when asking qwen3:32b to generate variations.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

RIOKIT_ROOT = Path.home() / "Documents/dev/packages/RIOExperimentationKit/Sources"
OUT_ROOT = Path(__file__).resolve().parent.parent / "data/raw/riokit"
OUT_ROOT.mkdir(parents=True, exist_ok=True)

# (rubric rule id, heuristic) — very simple regex heuristics. Intentionally
# imprecise: we tag liberally and let synth filter by fit.
RUBRIC_HEURISTICS: list[tuple[str, re.Pattern[str]]] = [
    ("R1", re.compile(r"^\s*public\s+typealias\s+\w+\s*=\s*\w+(\s*&\s*\w+)+", re.M)),
    ("R1", re.compile(r"^\s*public\s+protocol\s+\w+\s*:\s*Sendable\b", re.M)),
    ("R2", re.compile(r"@Observable\s+public\s+final\s+class\s+\w+\s*:\s*@unchecked\s+Sendable", re.M)),
    ("R2", re.compile(r"public\s+static\s+func\s+live\s*\(", re.M)),
    ("R2", re.compile(r"public\s+static\s+var\s+mock\s*:", re.M)),
    ("R2", re.compile(r"extension\s+\w+\s*:\s*\w+Managing\b", re.M)),
    ("R3", re.compile(r"^\s*public\s+actor\s+\w+", re.M)),
    ("R3", re.compile(r"nonisolated\s+public\s+let\s+\w+\s*:\s*AsyncStream", re.M)),
    ("R4", re.compile(r"@Entry\s+var\s+\w+", re.M)),
    ("R5", re.compile(r"public\s+final\s+class\s+Mock\w+\s*:\s*@unchecked\s+Sendable", re.M)),
    ("R7", re.compile(r"^//\s+Created by Christopher J Roura", re.M)),
    ("R7", re.compile(r"//\s*MARK:\s*-\s*Private\s+Properties", re.M)),
    ("R8", re.compile(r":\s*Sendable\b", re.M)),
    ("R9", re.compile(r"public\s+enum\s+\w+Error\s*:\s*Error", re.M)),
]


def tag_file(text: str) -> list[str]:
    """Return the rubric rule ids a file demonstrates."""
    tags: set[str] = set()
    for rule_id, pattern in RUBRIC_HEURISTICS:
        if pattern.search(text):
            tags.add(rule_id)
    return sorted(tags)


def main() -> None:
    if not RIOKIT_ROOT.exists():
        raise SystemExit(f"RIOKit not found at {RIOKIT_ROOT}")

    index: list[dict[str, object]] = []
    for swift_file in sorted(RIOKIT_ROOT.rglob("*.swift")):
        rel = swift_file.relative_to(RIOKIT_ROOT)
        out_path = OUT_ROOT / rel
        out_path.parent.mkdir(parents=True, exist_ok=True)
        text = swift_file.read_text(encoding="utf-8")
        out_path.write_text(text, encoding="utf-8")

        tags = tag_file(text)
        index.append({
            "source": "RIOExperimentationKit",
            "relative_path": str(rel),
            "line_count": len(text.splitlines()),
            "rubric_tags": tags,
        })

    index_path = OUT_ROOT / "_index.jsonl"
    with index_path.open("w", encoding="utf-8") as fh:
        for entry in index:
            fh.write(json.dumps(entry) + "\n")

    total = len(index)
    tag_counts: dict[str, int] = {}
    for entry in index:
        for t in entry["rubric_tags"]:  # type: ignore[index]
            tag_counts[t] = tag_counts.get(t, 0) + 1

    print(f"harvested {total} files from RIOKit → {OUT_ROOT}")
    for rule_id in sorted(tag_counts):
        print(f"  {rule_id}: {tag_counts[rule_id]} files")


if __name__ == "__main__":
    main()
