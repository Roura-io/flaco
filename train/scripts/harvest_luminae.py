#!/usr/bin/env python3
"""Harvest Luminae iOS app for architecture-pattern exemplars.

Luminae is Chris's production SwiftUI app and contains the canonical
examples of every pattern in the rubric applied at scale. We:

1. Walk every .swift file under Luminae/Luminae/
2. Tag each file with the rubric rules it demonstrates
3. Copy the files into data/raw/luminae/ preserving structure
4. Emit an index of which files are useful seeds for which rules

We DO NOT ship these files anywhere outside the local training run.
They stay in the .gitignored raw dir and are only read by
synthesize_pairs.py to build the instruction dataset.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

LUMINAE_ROOT = Path.home() / "Documents/current/luminae/ios.luminae/Luminae"
OUT_ROOT = Path(__file__).resolve().parent.parent / "data/raw/luminae"
OUT_ROOT.mkdir(parents=True, exist_ok=True)

RUBRIC_HEURISTICS: list[tuple[str, re.Pattern[str]]] = [
    ("R1", re.compile(r"^\s*(public\s+)?typealias\s+\w+\s*=\s*\w+(\s*&\s*\w+)+", re.M)),
    ("R1", re.compile(r"^\s*(public\s+)?protocol\s+\w+\s*:\s*Sendable\b", re.M)),
    ("R2", re.compile(r"@Observable\s+(public\s+)?final\s+class\s+\w+", re.M)),
    ("R2", re.compile(r"(public\s+)?static\s+func\s+live\s*\(", re.M)),
    ("R2", re.compile(r"(public\s+)?static\s+var\s+mock\s*:", re.M)),
    ("R3", re.compile(r"^\s*(public\s+)?actor\s+\w+", re.M)),
    ("R3", re.compile(r"nonisolated\s+(public\s+)?let\s+\w+\s*:\s*AsyncStream", re.M)),
    ("R4", re.compile(r"@Entry\s+var\s+\w+", re.M)),
    ("R4", re.compile(r"\.environment\(\\\.\w+,", re.M)),
    ("R4", re.compile(r"@Environment\(\\\.\w+\)", re.M)),
    ("R4", re.compile(r"_viewModel\s*=\s*State\(wrappedValue:", re.M)),
    ("R5", re.compile(r"(public\s+)?final\s+class\s+Mock\w+", re.M)),
    ("R7", re.compile(r"//\s*MARK:\s*-\s*Private\s+Properties", re.M)),
    ("R9", re.compile(r"(public\s+)?enum\s+\w+Error\s*:\s*Error", re.M)),
    ("R11", re.compile(r"class\s+\w*Presenter\b", re.M)),
    ("R11", re.compile(r"class\s+\w*Interactor\b", re.M)),
    ("R11", re.compile(r"class\s+\w*Router\b", re.M)),
]


def tag_file(text: str) -> list[str]:
    tags: set[str] = set()
    for rule_id, pattern in RUBRIC_HEURISTICS:
        if pattern.search(text):
            tags.add(rule_id)
    return sorted(tags)


def main() -> None:
    if not LUMINAE_ROOT.exists():
        raise SystemExit(f"Luminae not found at {LUMINAE_ROOT}")

    index: list[dict[str, object]] = []
    skipped_binary = 0

    for swift_file in sorted(LUMINAE_ROOT.rglob("*.swift")):
        rel = swift_file.relative_to(LUMINAE_ROOT)
        out_path = OUT_ROOT / rel
        out_path.parent.mkdir(parents=True, exist_ok=True)
        try:
            text = swift_file.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            skipped_binary += 1
            continue

        out_path.write_text(text, encoding="utf-8")
        tags = tag_file(text)
        index.append({
            "source": "Luminae",
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

    print(f"harvested {total} swift files from Luminae → {OUT_ROOT}")
    if skipped_binary:
        print(f"  (skipped {skipped_binary} non-utf8 files)")
    for rule_id in sorted(tag_counts):
        print(f"  {rule_id}: {tag_counts[rule_id]} files")


if __name__ == "__main__":
    main()
