#!/usr/bin/env python3
"""Expand hand-written seeds into a full training dataset using qwen3:32b
via the local Ollama API.

Input:
  data/seeds/architecture_seeds.jsonl    — hand-written rubric-approved pairs
  data/seeds/chris_voice_raw.jsonl       — harvested Chris assistant replies
  data/seeds/walter_voice_raw.jsonl      — harvested Walter-tagged replies
  data/seeds/chris_memory_facts.jsonl    — claude memory fact bodies
  data/raw/riokit/**/*.swift             — RIOKit positive exemplars
  data/raw/luminae/**/*.swift            — Luminae positive exemplars (tagged)

Process (per batch):
  1. Pick a seed (or an exemplar file).
  2. Ask qwen3:32b: "generate 3 instruction/response training pairs
     that demonstrate the same rubric rule, varying the service type,
     domain, and component name. The response must conform to the rubric."
  3. The system prompt embeds the full rubric.md so the teacher model
     is grounded in it.
  4. Parse the response, validate it has the expected keys, write to
     pairs/train.jsonl.

Output:
  data/pairs/train.jsonl       — one {messages: [...]} per line, ready
                                 for the Colab training notebook.

Runtime:
  ~5 hours on an M1 Pro using qwen3:32b-q8_0. Checkpointed — safe to ^C
  and restart; it appends to pairs/train.jsonl and a progress file.

Why a teacher model at all:
  Hand-writing 25k pairs is infeasible. The teacher model just rephrases
  and varies the few hundred rubric-approved seeds. Every output is
  spot-checked by running build_dataset.py which re-validates against
  the rubric heuristics before the training data leaves this machine.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

try:
    import requests
except ImportError:
    print("This script needs the 'requests' package:")
    print("  python3 -m pip install --user requests")
    sys.exit(1)


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
RUBRIC = (ROOT / "rubric.md").read_text(encoding="utf-8")

OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://localhost:11434")
TEACHER_MODEL = os.environ.get("FLACO_TEACHER_MODEL", "qwen3:32b-q8_0")

SEEDS_DIR = ROOT / "data/seeds"
PAIRS_DIR = ROOT / "data/pairs"
PAIRS_DIR.mkdir(parents=True, exist_ok=True)
PAIRS_PATH = PAIRS_DIR / "train.jsonl"
PROGRESS_PATH = PAIRS_DIR / ".progress.json"


SYSTEM_PROMPT = (
    "You are the teacher model for flacoAi's custom LoRA training run. "
    "Your only job is to produce instruction/response training pairs that "
    "conform to Chris Roura's architecture rubric (provided below). "
    "Every pair you emit MUST match the rubric exactly — if you can't, "
    "skip and say so. Output JSON only. No prose, no preamble.\n\n"
    "OUTPUT FORMAT: a JSON array of objects, each with keys "
    "`prompt` (what a user would ask) and `response` (the rubric-conformant "
    "answer). Nothing else. Example:\n"
    '[{"prompt":"…","response":"…"},{"prompt":"…","response":"…"}]\n\n'
    "THE RUBRIC (study before generating):\n\n" + RUBRIC
)


def call_teacher(user_prompt: str, temperature: float = 0.6) -> str:
    body = {
        "model": TEACHER_MODEL,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ],
        "stream": False,
        "options": {"temperature": temperature, "num_ctx": 16384},
    }
    resp = requests.post(f"{OLLAMA_URL}/api/chat", json=body, timeout=600)
    resp.raise_for_status()
    data = resp.json()
    return data["message"]["content"]


def try_parse_pairs(raw: str) -> list[dict[str, str]]:
    """Find the first [...] block and json-parse it."""
    start = raw.find("[")
    end = raw.rfind("]")
    if start < 0 or end < 0 or end < start:
        return []
    chunk = raw[start : end + 1]
    try:
        parsed = json.loads(chunk)
    except json.JSONDecodeError:
        return []
    out: list[dict[str, str]] = []
    for row in parsed:
        if not isinstance(row, dict):
            continue
        p = row.get("prompt")
        r = row.get("response")
        if isinstance(p, str) and isinstance(r, str) and p.strip() and r.strip():
            out.append({"prompt": p.strip(), "response": r.strip()})
    return out


def messages_from_pair(pair: dict[str, str], kind: str) -> dict[str, Any]:
    """Wrap a raw pair in the chat-template format the trainer expects."""
    system = (
        "You are flacoAi, a local-only AI brain for Chris Roura "
        "(Roura.io). You are expert in Swift 6, SwiftUI, strict "
        "concurrency, and Chris's protocol-oriented architecture. "
        "You follow his rubric exactly — init DI for VM-backed views, "
        "@Entry + environment as fallback, actor wrappers for I/O, "
        "manager facades with .live/.mock factories, MARK-sectioned "
        "files, dedicated error enums. Never use @EnvironmentObject, "
        "singletons, @StateObject with parameterless init, or "
        "completion-handler APIs."
    )
    return {
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": pair["prompt"]},
            {"role": "assistant", "content": pair["response"]},
        ],
        "meta": {"kind": kind, "source": "synthesized"},
    }


def load_seeds() -> list[dict[str, Any]]:
    seeds_path = SEEDS_DIR / "architecture_seeds.jsonl"
    if not seeds_path.exists():
        raise SystemExit(f"no seeds at {seeds_path}")
    seeds: list[dict[str, Any]] = []
    with seeds_path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            seeds.append(json.loads(line))
    return seeds


def save_progress(progress: dict[str, Any]) -> None:
    PROGRESS_PATH.write_text(json.dumps(progress, indent=2), encoding="utf-8")


def load_progress() -> dict[str, Any]:
    if PROGRESS_PATH.exists():
        return json.loads(PROGRESS_PATH.read_text(encoding="utf-8"))
    return {"seed_idx": 0, "variations_per_seed_done": {}}


def expand_seed(seed: dict[str, Any], target: int) -> list[dict[str, Any]]:
    """Ask the teacher to produce `target` variations of a seed."""
    rule = seed.get("rule", "?")
    user_prompt = (
        f"Seed (rule {rule}):\n"
        f"PROMPT: {seed['prompt']}\n\n"
        f"RESPONSE:\n{seed['ideal_response']}\n\n"
        f"Task: generate {target} NEW instruction/response pairs that "
        f"demonstrate the same rubric rule ({rule}) with different "
        f"service types, domains, component names, and feature areas. "
        f"Keep the style identical to the seed response. "
        f"Return ONLY the JSON array, no commentary."
    )
    raw = call_teacher(user_prompt)
    return try_parse_pairs(raw)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--variations-per-seed", type=int, default=20,
                        help="how many new pairs to generate per seed")
    parser.add_argument("--max-seeds", type=int, default=0,
                        help="limit for smoke tests, 0 = all")
    parser.add_argument("--dry-run", action="store_true",
                        help="don't call the teacher, just print plan")
    args = parser.parse_args()

    seeds = load_seeds()
    if args.max_seeds:
        seeds = seeds[: args.max_seeds]

    progress = load_progress()
    appended = 0

    with PAIRS_PATH.open("a", encoding="utf-8") as out_fh:
        for idx, seed in enumerate(seeds):
            if idx < progress["seed_idx"]:
                continue
            already = progress["variations_per_seed_done"].get(str(idx), 0)
            need = max(args.variations_per_seed - already, 0)
            if need == 0:
                continue

            label = f"[{idx+1}/{len(seeds)}] rule={seed.get('rule','?')}"
            if args.dry_run:
                print(f"{label} would request {need} variations")
                continue

            started = time.time()
            try:
                pairs = expand_seed(seed, need)
            except requests.RequestException as e:
                print(f"{label} teacher call failed: {e}")
                time.sleep(3)
                continue

            for pair in pairs:
                row = messages_from_pair(pair, kind=seed.get("kind", "architecture"))
                row["meta"]["rubric_rule"] = seed.get("rule")
                row["meta"]["from_seed"] = idx
                out_fh.write(json.dumps(row, ensure_ascii=False) + "\n")
                appended += 1

            progress["seed_idx"] = idx + 1
            progress["variations_per_seed_done"][str(idx)] = already + len(pairs)
            save_progress(progress)
            out_fh.flush()

            dur = time.time() - started
            print(f"{label} → {len(pairs)} pairs ({dur:.1f}s, total appended={appended})")

    print(f"done. {appended} new pairs appended to {PAIRS_PATH}")


if __name__ == "__main__":
    main()
