#!/usr/bin/env python3
"""Run the eval holdout against any Ollama model and score pass/fail.

Usage:
  python3 eval_model.py --model qwen3:32b-q8_0
  python3 eval_model.py --model flaco-custom:7b
  python3 eval_model.py --model qwen3:32b-q8_0 --model flaco-custom:7b   # compare

Scoring:
  Each holdout case has `expect_markers` (strings that must appear in the
  response) and optionally `avoid_markers` (strings that must NOT appear).
  A case passes if ALL expect_markers are present AND zero avoid_markers
  are present. Per-rule breakdown is printed.

This is coarse on purpose — the goal is a fast regression check that
catches "the model stopped emitting @Entry" or "the model started
recommending @EnvironmentObject again," not semantic grading.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

try:
    import requests
except ImportError:
    print("needs `pip install requests`")
    sys.exit(1)


HERE = Path(__file__).resolve().parent
EVAL_PATH = HERE.parent / "data/eval/holdout.jsonl"
OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://localhost:11434")

SYSTEM_PROMPT = (
    "You are flacoAi, a local-only AI brain for Chris Roura (Roura.io). "
    "Expert in Swift 6, SwiftUI, strict concurrency, and Chris's "
    "protocol-oriented architecture rubric. Follow the rubric exactly. "
    "Never use @EnvironmentObject, singletons, @StateObject with "
    "parameterless init, or completion-handler APIs."
)


def ollama_chat(model: str, user: str) -> str:
    body = {
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user},
        ],
        "stream": False,
        "options": {"temperature": 0.2, "num_ctx": 4096},
    }
    r = requests.post(f"{OLLAMA_URL}/api/chat", json=body, timeout=600)
    r.raise_for_status()
    return r.json()["message"]["content"]


def load_cases() -> list[dict]:
    with EVAL_PATH.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def score_case(case: dict, response: str) -> tuple[bool, str]:
    expect = case.get("expect_markers", [])
    avoid = case.get("avoid_markers", [])
    missing = [m for m in expect if m not in response]
    bad = [m for m in avoid if m in response]
    if not missing and not bad:
        return True, "ok"
    reasons = []
    if missing:
        reasons.append(f"missing={missing}")
    if bad:
        reasons.append(f"contains_forbidden={bad}")
    return False, " ".join(reasons)


def run_one(model: str, cases: list[dict]) -> dict:
    by_rule: dict[str, list[bool]] = {}
    results: list[dict] = []
    for case in cases:
        try:
            resp = ollama_chat(model, case["prompt"])
        except requests.RequestException as e:
            results.append({
                "id": case["id"],
                "passed": False,
                "error": f"request failed: {e}",
                "rule": case.get("rule"),
            })
            continue
        passed, detail = score_case(case, resp)
        rule = case.get("rule") or "misc"
        by_rule.setdefault(rule, []).append(passed)
        results.append({
            "id": case["id"],
            "passed": passed,
            "detail": detail,
            "rule": rule,
            "response_preview": resp[:300],
        })
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {case['id']:<12}  {detail}")

    total = len(results)
    passed = sum(1 for r in results if r["passed"])
    print()
    print(f"  {model}: {passed}/{total} passed")
    print("  by rule:")
    for rule in sorted(by_rule):
        bs = by_rule[rule]
        print(f"    {rule}: {sum(bs)}/{len(bs)}")

    return {
        "model": model,
        "total": total,
        "passed": passed,
        "by_rule": {k: (sum(v), len(v)) for k, v in by_rule.items()},
        "results": results,
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", action="append", required=True,
                    help="ollama model name (can repeat for comparison)")
    ap.add_argument("--out", default=str(HERE.parent / "data/eval/last_run.json"))
    args = ap.parse_args()

    cases = load_cases()
    print(f"loaded {len(cases)} eval cases\n")

    all_runs = []
    for model in args.model:
        print(f"=== {model} ===")
        run = run_one(model, cases)
        all_runs.append(run)
        print()

    Path(args.out).write_text(json.dumps(all_runs, indent=2), encoding="utf-8")
    print(f"detailed results → {args.out}")

    if len(all_runs) > 1:
        print("\ncomparison:")
        for run in all_runs:
            print(f"  {run['model']:<30} {run['passed']:>3}/{run['total']}")


if __name__ == "__main__":
    main()
