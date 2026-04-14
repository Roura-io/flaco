#!/usr/bin/env python3
"""Harvest Chris-voice and Walter-voice anchors from flaco memory.

Sources:
- ~/infra/flaco.db (or local equivalent) — unified flacoAi memory
- ~/.claude/projects/<slug>/memory/*.md — the claude auto-memory facts

For Chris:
  - Assistant replies in the flaco message history that came back from
    web / TUI / CLI conversations (these are in his voice because they
    were validated by him not complaining about them).
  - The body of every claude auto-memory fact file.

For Walter:
  - Assistant replies in conversations with surface=slack AND
    user_id=Walter (if any) — the already-approved Walter voice.
  - Assistant replies in conversations that happened in #dad-help.
    (Approximated by channel name captured on the conversation row.)

Output:
  data/seeds/chris_voice_raw.jsonl
  data/seeds/walter_voice_raw.jsonl

These get hand-filtered later and used as seeds for the synth pipeline.
"""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parent / "data/seeds"
OUT.mkdir(parents=True, exist_ok=True)

# Candidate locations for the flaco memory db — check in order.
DB_CANDIDATES = [
    Path.home() / "infra/flaco.db",
    Path.home() / "Documents/dev/flacoAi/flaco.db",
    Path("/Users/roura.io.server/infra/flaco.db"),
]

CLAUDE_MEMORY_DIR = Path.home() / ".claude/projects/-Users-roura-io-Documents-pi-projects/memory"


def find_db() -> Path | None:
    for candidate in DB_CANDIDATES:
        if candidate.exists():
            return candidate
    return None


def harvest_db(db_path: Path) -> tuple[list[dict], list[dict]]:
    """Return (chris_examples, walter_examples)."""
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    # Look at conversations + messages. The schema has:
    #   conversations(id, surface, user_id, persona, title, created_at, updated_at)
    #   messages(id, conversation_id, role, content, tool_name, created_at)
    cur.execute("""
        SELECT m.content, m.role, c.surface, c.user_id, c.persona, c.id AS conv_id
        FROM messages m
        JOIN conversations c ON m.conversation_id = c.id
        WHERE m.role = 'assistant'
          AND length(m.content) > 30
        ORDER BY m.created_at DESC
        LIMIT 2000
    """)

    chris: list[dict] = []
    walter: list[dict] = []
    for row in cur.fetchall():
        entry = {
            "content": row["content"],
            "surface": row["surface"],
            "user_id": row["user_id"],
            "persona": row["persona"],
            "conv_id": row["conv_id"],
        }
        persona = (row["persona"] or "").lower()
        if "walter" in persona or "dad" in persona:
            walter.append(entry)
        else:
            chris.append(entry)

    conn.close()
    return chris, walter


def harvest_claude_memory() -> list[dict]:
    if not CLAUDE_MEMORY_DIR.exists():
        return []
    out: list[dict] = []
    for md in sorted(CLAUDE_MEMORY_DIR.glob("*.md")):
        if md.name == "MEMORY.md":
            continue
        text = md.read_text(encoding="utf-8")
        if text.strip().startswith("---"):
            parts = text.split("---", 2)
            if len(parts) == 3:
                text = parts[2]
        out.append({
            "source_file": md.name,
            "content": text.strip(),
        })
    return out


def write_jsonl(path: Path, rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")


def main() -> None:
    db = find_db()
    chris_count = walter_count = 0
    if db is None:
        print("flaco.db not found locally — skipping db harvest.")
        print("  If it lives on mac-server, rsync it first:")
        print("  rsync mac-server:~/infra/flaco.db ~/infra/flaco.db")
    else:
        print(f"reading {db}")
        chris, walter = harvest_db(db)
        write_jsonl(OUT / "chris_voice_raw.jsonl", chris)
        write_jsonl(OUT / "walter_voice_raw.jsonl", walter)
        chris_count = len(chris)
        walter_count = len(walter)

    memory_facts = harvest_claude_memory()
    write_jsonl(OUT / "chris_memory_facts.jsonl", memory_facts)

    print(f"  chris_voice_raw   → {chris_count} examples")
    print(f"  walter_voice_raw  → {walter_count} examples")
    print(f"  chris_memory_facts → {len(memory_facts)} facts")


if __name__ == "__main__":
    main()
