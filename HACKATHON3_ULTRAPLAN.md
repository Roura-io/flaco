# flacoAi — Hackathon 3 ULTRAPLAN (v2, execution-ready)

> **Branch:** `feature/hackathon-3-training`
> **Start:** 2026-04-13 afternoon ET
> **Target finish:** ~19 hours of focused engineering + 5 hours unattended Colab compute + 25 min of Chris's time
> **Stance:** every item in this plan is a commit. No item is "done" until I've executed the verification command and the output matches the claim.

This document is designed for deterministic execution by a downstream agent with no external context. Every step has concrete file paths, concrete commands, and concrete pass/fail criteria.

---

## 1. Scope Summary

Close all four v2 grader ship-blockers on flacoAi v2 (A1 tool-loop dedup already shipped and live-verified in commit `ef761d8`; A2 backups, A3 launchd supervisor, A4 config crate remain). Add a tiered tool-store so Chris gets every tool and default/OSS users get a credential-free subset they can opt into. Ship an `install-home.sh` for Chris's Mac server and a vendor-agnostic `install.sh` for anyone else. Finish the custom LoRA training pipeline scaffolded in commits `4b…` and `c85a895` — gated on two short human-in-the-loop interactions with Chris (rubric review + seed interview). Ship grader nice-to-haves (SSE, real-Ollama integration test, pre-existing test fixes, clippy, `/grade/*` endpoints) if time allows. All work on the feature branch; v1 `flacoai-server` and Walter's n8n workflows are untouched throughout.

## 2. Ultraplan (Execution Plan)

### P0 — Must ship (~4.25 h remaining)

| ID | Task | Time | Verification | Deps |
|---|---|---|---|---|
| A1 | ✅ Tool-loop dedup (runtime HashSet + tool-level idempotency) | 0 | `ef761d8` live-verified | — |
| A4 | `flaco-config` crate + `config.toml` loader + extract hardcoded paths | 1.5 h | `cargo test -p flaco-config` = 3 pass; `flaco-v2 --config /tmp/alt.toml status` reflects override | A1 |
| A2 | `flaco-backup.sh` + `io.roura.flaco.backup.plist` (launchd 03:00 daily) | 0.75 h | `launchctl print gui/$UID/io.roura.flaco.backup` shows state; manual kickstart produces `~/Documents/flaco-backups/flaco-*.db` | A4 |
| A3 | `io.roura.flaco.plist` launchd supervisor for `flaco-v2 web` | 0.75 h | kill the pid; 12 s later `curl http://localhost:3033/health` returns `ok` | A4 |
| B4 | `flaco-v2 doctor` subcommand (8 checks, colored output, non-zero on red) | 1.25 h | `flaco-v2 doctor` green; `FLACO_OLLAMA_URL=bogus flaco-v2 doctor; echo $?` non-zero | A2 A3 A4 |

**Track P0 total: 4.25 h.**

### P1 — Important (~11.25 h engineering + 25 min Chris + 5 h compute)

| ID | Task | Time | Verification | Deps |
|---|---|---|---|---|
| B1 | Tool-store `ToolManifest` + `Tier` enum + `ToolRegistry::effective_for` + `GET /tools` endpoint | 3.5 h | `curl /tools?tier=default \| jq length` < `curl /tools?tier=chris \| jq length`; new test `tool_manifest_filters_by_tier_and_env` passes | A4 |
| B2 | `install-home.sh` (Chris's Mac, idempotent, launchd install, runs `flaco doctor` at end) | 2 h | `bash install-home.sh --dry-run` enumerates ops; 2nd real run exits 0 | A2 A3 A4 B4 |
| B3 | `install.sh` for OSS (detect OS, check Ollama, write tier=default config, no credentials required) | 1.75 h | `docker run ... ubuntu:22.04 bash install.sh --non-interactive` exits 0, `/etc/flaco/config.toml` has `tier = "default"` | B1 A4 |
| C1 | Rubric review gate (**GATE: 10 min Chris**) | 0 | Chris replies "approved", I remove review-checklist section | — |
| C2 | Seed interview (write 40 prompts, **GATE: 15 min Chris**, parse answers to JSONL) | 0.5 h + Chris | `wc -l train/data/seeds/seed_pairs.jsonl` ≥ 40 | C1 |
| C3 | Full synthesis run (`synthesize_pairs.py --variations-per-seed 50` → `build_dataset.py`) | 0.5 h setup + 5 h compute unattended | `wc -l train.merged.jsonl` ≥ 1500; `_total_dropped` < 10% | C2 |
| C4 | Colab LoRA training (Qwen2.5-Coder-7B + Unsloth + QLoRA, T4, ~2 h wall-clock) | 2.5 h | `tar -tzf flaco-custom-lora.tar.gz` has `adapter_model`; `ollama list \| grep flaco-custom:7b` | C3 |
| C5 | Router heuristic + eval regression (regex-based model selection in `runtime::handle_turn`) | 1.5 h | `eval_model.py` custom > baseline on R4 bucket; chat returns `@Entry` code | C4 |
| B5 | `GRADING3.md` reviewer guide | 1.25 h | `grep -c '^##'` ≥ 10; `grep -cE 'curl\|ssh\|cargo'` ≥ 25 | all above |
| B6 | Slack DM + `#infra-status` post | 0.5 h | `chat.postMessage` `ok:true` for both | B5 |

**Track P1 total: 11.25 h engineering + 5 h compute + 25 min Chris.**

### P2 — Nice to have (~3.75 h)

| ID | Task | Time | Verification | Deps |
|---|---|---|---|---|
| D1 | SSE streaming for `/chat` and `/research` (HTMX `hx-ext="sse"`) | 1.25 h | `curl -N /chat-stream?m=hi` yields ≥2 `data:` frames over ≥2 s + terminal `event: done` | A4 |
| D2 | Real-Ollama integration test (`#[ignore]` default) | 0.5 h | `cargo test -- --ignored ollama_reachable_smoke` green | — |
| D3 | Fix 3 pre-existing test failures on `main` | 0.75 h | `cargo test --workspace \| grep "0 failed"` | — |
| D4 | `cargo clippy --fix -p flaco-core` + `-D warnings` | 0.25 h | `cargo clippy -p flaco-core -- -D warnings` clean | D3 |
| D5 | `/grade/tests`, `/grade/features`, `/grade/commits-since-v2` endpoints | 1 h | `curl /grade/features \| jq '.[].status' \| sort -u` yields `up/degraded/down` | A4 |

**Track P2 total: 3.75 h.**

### P3 — Optional (deferred)

Pruning, pagination, audit UI, multi-user auth, HTTPS, CSRF, rate limiting, vector recall, persona hot-reload, dry-run flag. All in §9 non-goals with a reason.

## 3. Technical Decisions

| Decision | Choice | Why (1–2 sentences) | Alternative considered |
|---|---|---|---|
| Config format | TOML via `toml` + `serde` derive | Native Rust, comment-friendly, same stack as `Cargo.toml`. | JSON (no comments), YAML (whitespace fragility). |
| Config location | `/opt/homebrew/etc/flaco/config.toml`, env override `FLACO_CONFIG_PATH` | Matches grader's explicit prescription and Homebrew conventions. | `~/.config/flaco` would break launchd working-dir assumptions. |
| Supervisor | `launchctl bootstrap gui/$UID` + `KeepAlive { Crashed: true, ThrottleInterval: 10 }` | Native macOS, survives reboot, captures stdio, no extra dependencies. | `brew services` (thin wrapper, less control), systemd (wrong OS). |
| Backup | `sqlite3 $DB "VACUUM INTO '$OUT'"` in a launchd calendar-interval timer | Online, consistent, no lock, atomic; works on a live DB. | `cp` unsafe on live DB; `.dump` 10× larger. |
| Tool tier model | Compile-time `ToolManifest { tier, requires_env, default_for_tiers }` per tool + `ToolRegistry::effective_for(tier)` runtime filter | Static enum is typo-proof and zero-alloc; expressive enough for the three audiences. | Runtime tag strings (typo-prone), RBAC (overkill). |
| LLM base | `unsloth/Qwen2.5-Coder-7B-Instruct-bnb-4bit` | Best reasoning-per-byte in the 7B code-specialist class, Apache 2.0, trains ~2 h on a free Colab T4 via Unsloth. | Llama-3.1-8B (weaker code), DeepSeek-Coder-6.7B (less active). |
| LoRA hyperparams | rank 32, alpha 32, dropout 0.05, 3 epochs, `adamw_8bit`, lr 2e-4 cosine | Sweet spot — high enough to encode R1-R11, low enough for T4 VRAM. | rank 16 under-fits; rank 64 OOMs at 2048 seq len. |
| Training platform | Google Colab free T4 (Kaggle T4×2 backup) | Free, zero setup, Mac Studio M3 Ultra not available until mid-May. | Local MLX on M1 Pro would take 10-14 h per epoch. |
| Router | Regex heuristic on user message → `flaco-custom:7b` else default | ~20 LOC, zero new deps, fully local, easy to extend. | LLM-based routing adds a hop and latency. |
| Language | Rust everywhere except `train/scripts/*.py` (Unsloth requires Python) | Match existing stack. | — |

## 4. Tradeoffs

**Optimizes for:** local-only single-user reliability, small reviewable commits, per-step verifiability, zero-credential onboarding for OSS users, feature parity across Slack/TUI/Web through `flaco-core`, commit-after-each-item visibility in `git log`.

**Sacrifices:** multi-user identity, HTTPS, cloud sync, frontier-model quality on general reasoning, hot-reloadable config, online schema migration, observability beyond one Uptime Kuma health probe, dry-run for destructive tools. All explicitly chosen, see §9.

## 5. Definition of Done

v3 is done when **every** one of these is simultaneously true on `feature/hackathon-3-training`:

1. `cargo test --workspace --release` — 0 failed (including the 3 pre-existing failures fixed in D3).
2. `ssh mac-server '~/infra/flaco-v2 doctor'` — all checks PASS, exit 0.
3. `launchctl print gui/$(id -u)/io.roura.flaco` — state `running`.
4. `launchctl print gui/$(id -u)/io.roura.flaco.backup` — state `waiting` or `running`.
5. `~/Documents/flaco-backups/` has ≥1 `flaco-*.db` modified in the last 25 h, and `sqlite3 $file "SELECT count(*) FROM memories"` returns non-zero.
6. `curl -s http://mac.home:3033/tools?tier=default | jq length` < `curl -s http://mac.home:3033/tools?tier=chris | jq length`.
7. `curl -sS -X POST http://mac.home:3033/chat --data-urlencode 'message=Write a SwiftUI view with @Entry environment injection' | grep -q '@Entry'` — pass.
8. `ollama list | grep flaco-custom:7b` — present.
9. `python3 train/scripts/eval_model.py --model qwen3:32b-q8_0 --model flaco-custom:7b` — `flaco-custom:7b` wins ≥70% of R4 cases.
10. `docker run --rm -v $PWD:/flaco ubuntu:22.04 bash /flaco/deploy/install.sh --non-interactive` exits 0.
11. `ssh mac-server 'bash ~/infra/install-home.sh && ~/infra/flaco-v2 doctor'` — all green, idempotent (re-run also exits 0).
12. `GRADING3.md` exists, has ≥10 `##` sections and ≥25 executable commands.
13. `#infra-status` Slack channel has a v3 shipped post; `elGordoRoura` has received the DM.
14. v1 `flacoai-server` is still running; Walter's 6 n8n workflows are still inactive.

## 6. End-to-End Verification

Single script `deploy/smoke-v3.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
HOST=${HOST:-mac.home}
BASE="http://$HOST:3033"
fail() { echo "FAIL: $*" >&2; exit 1; }

curl -fsS "$BASE/health" | grep -q ok                                              || fail "health"
[ "$(curl -fsS "$BASE/status" | jq '.tools | length')" -ge 15 ]                    || fail "tools<15"
ssh "$HOST" "launchctl print gui/\$(id -u)/io.roura.flaco"        | grep -q state  || fail "launchd web"
ssh "$HOST" "launchctl print gui/\$(id -u)/io.roura.flaco.backup" | grep -q state  || fail "launchd backup"
ssh "$HOST" 'find ~/Documents/flaco-backups -name "flaco-*.db" -mmin -1500 | head -1 | grep -q flaco' || fail "recent backup"
D=$(curl -fsS "$BASE/tools?tier=default" | jq length)
C=$(curl -fsS "$BASE/tools?tier=chris"   | jq length)
[ "$D" -lt "$C" ] || fail "tier filter ($D >= $C)"
curl -fsS -X POST "$BASE/chat" --data-urlencode 'message=SwiftUI view with @Entry injection' | grep -q '@Entry' || fail "chat @Entry"
ssh "$HOST" 'ollama list' | grep -q flaco-custom:7b                                || fail "flaco-custom:7b missing"
python3 train/scripts/eval_model.py --model qwen3:32b-q8_0 --model flaco-custom:7b --out /tmp/eval.json >/dev/null
python3 -c "import json; r=json.load(open('/tmp/eval.json')); assert r[1]['by_rule'].get('R4',[0,1])[0] >= r[0]['by_rule'].get('R4',[0,1])[0]" || fail "eval R4"
ssh "$HOST" 'ps -ef | grep -v grep | grep -q flacoai-server'                       || fail "v1 dead"
echo ALL GREEN
```

Exit 0 from `bash deploy/smoke-v3.sh` is the single canonical "v3 shipped" signal.

## 7. Risks & Unknowns

1. **Colab T4 session disconnects mid-training.**
   - Why it matters: lose 60-90 min of compute, maybe a half-trained adapter.
   - Detect: notebook last printed step count vs expected total.
   - Mitigate: `save_steps=100` + `save_total_limit=3` already set in the notebook; resume by uploading the last checkpoint and continuing. Fallback: Kaggle T4×2 has 30 h/week and longer sessions.

2. **qwen3:32b teacher drifts from the rubric during synthesis.**
   - Why it matters: garbage training data produces a garbage LoRA.
   - Detect: `build_dataset.py` regex-validates each row against rubric markers and prints drop count.
   - Mitigate: if `_total_dropped > 20%`, halve temperature (0.6 → 0.3), re-run, manually inspect 20 random kept rows.

3. **Router regex fires on false positives.**
   - Why it matters: `flaco-custom:7b` gets picked for non-Swift questions where it'll perform worse than qwen3.
   - Detect: log `router.selected_model` on every turn; grep for `flaco-custom:7b` on non-Swift messages in the log.
   - Mitigate: case-sensitive match on `@Observable`/`@Entry` (Swift-only tokens), word-boundary on `SwiftUI`/`ViewModel`. If still noisy, add a token-count threshold (must contain ≥2 Swift markers).

## 8. Execution Order / Critical Path

```
P0 (strictly sequential — A2/A3/B4 all read config produced by A4):
  A1 ✅ → A4 → A2 → A3 → B4

P1 parallelizes after A4:
  A4 → B1 ─┬─→ B2 ─┐
           └─→ B3 ─┤
  C1 (user) → C2 (user) → C3 ─→ C4 ─→ C5 ──┴─→ B5 → B6
```

Critical path (longest hours): A4 (1.5) → A2 (0.75) → A3 (0.75) → B4 (1.25) → B1 (3.5) → B2 (2) → B5 (1.25) → B6 (0.5) = **11.5 h of sequential engineering**. C track compute (5 h) runs unattended during B track engineering. P2 items are independently schedulable.

Parallelizable: A2 ∥ A3, B2 ∥ B3, D1 ∥ D2 ∥ D3.

## 9. Non-Goals

| Non-goal | Reason |
|---|---|
| Auth / sessions / HTTPS / CSRF / rate limiting | Single-user LAN homelab; grader explicitly descoped all five. |
| Multi-user identity | No second user until Walter onboards via Slack, which already has its own auth. |
| HIPAA compliance | Personal infra, no regulated data flows. |
| Schema migrations framework | Defer until the first real schema change; pay the cost once. |
| Observability beyond Uptime Kuma health check | Grader explicitly descoped. |
| Conversation / memory pruning | No scale pressure yet (<10 MB db). |
| Pagination on list endpoints | <200 rows everywhere. |
| Vector recall via `nomic-embed-text` | FTS5 is enough for current volume; drop-in upgrade later. |
| Persona hot-reload | One persona in use; defer until there are two. |
| `--dry-run` flag for destructive tools | Grader flagged as nice-to-have; not a blocker. |
| Retrain on Mac Studio M3 Ultra | Hardware arrives mid-May; hackathon 4 target. |
| Slack v2 cutover | v1 keeps the Socket Mode token; v2 runs shadow mode only. |

## 10. Assumptions

1. `mac-server` is reachable via `ssh mac-server` as `roura.io.server`; Ollama runs at `http://localhost:11434` with `qwen3:32b-q8_0` pulled.
2. Chris has Colab free-tier access; T4 allocation works without a paid tier.
3. Chris's 25 min budget is spent at two gates (rubric review + seed interview); any redirects happen between commits, not mid-task.
4. `feature/hackathon-3-training` stays the working branch; no merge to `main` until review.
5. v1 `flacoai-server` continues to own the Slack Socket Mode token; v2 Slack adapter runs in shadow-mode only.
6. `/Volumes/unas/backups/` may not be mounted; fallback backup destination is `~/Documents/flaco-backups/`.
7. `/opt/homebrew/etc/flaco/` is writable by the Mac user running `install-home.sh`.
8. `~/Documents/current/luminae/ios.luminae/` is stable; its `.swift` files are the canonical positive architecture examples.
9. Walter has no new `#dad-help` messages since v2; Walter-voice training data is synthetic only (confirmed with Chris).
10. Mac launchd runs under `gui/$(id -u)` (user launch agents), not `system/`.

## 11. Executor Instructions (Claude CLI Runbook)

*(The full, linear runbook with file-level contents and commands is in the response body directly above this plan — reproduced here without commentary for machine execution.)*

### STEP 0 — confirm starting state
```bash
cd /Users/roura.io/Documents/dev/flacoAi
git status --short
git log --oneline -3
git branch --show-current
ssh mac-server 'curl -sS http://localhost:3033/health'
```
Expect: clean tree, `ef761d8` at HEAD, branch `feature/hackathon-3-training`, `ok` from health.

### STEP A4.1 — scaffold flaco-config crate
```bash
mkdir -p rust/crates/flaco-config/src
```
Write `rust/crates/flaco-config/Cargo.toml` and `rust/crates/flaco-config/src/lib.rs` as specified in the response runbook.
Add `"crates/flaco-config"` to `rust/Cargo.toml` `[workspace] members`.
Verify: `cd rust && cargo check -p flaco-config`.

### STEP A4.2 — tests for flaco-config
Three `#[test]` functions in `lib.rs`: `default_loads`, `env_overrides_file`, `file_overrides_default`.
Verify: `cargo test -p flaco-config` = `3 passed`.

### STEP A4.3 — replace hardcoded paths in flaco-v2
Add `--config <PATH>` CLI flag. Pass loaded `Config` into `build_runtime`, `serve_web`, shortcut tool constructor.
Verify: `cargo check -p flaco-v2` clean; `flaco-v2 --config /tmp/alt-flaco.toml status | grep /tmp/alt-flaco.db`.

### STEP A4.4 — build + commit A4
```bash
cd rust && cargo build --release -p flaco-v2
cd .. && git add rust/crates/flaco-config rust/crates/flaco-v2 rust/Cargo.toml rust/Cargo.lock
git commit -m "hackathon-3 A4: flaco-config crate + hardcoded path extraction"
```

### STEP A2 — backup script + launchd timer
Write `deploy/flaco-backup.sh`, `deploy/io.roura.flaco.backup.plist` (full content in response body).
```bash
scp -q deploy/flaco-backup.sh mac-server:~/infra/flaco-backup.sh
scp -q deploy/io.roura.flaco.backup.plist mac-server:~/Library/LaunchAgents/
ssh mac-server 'mkdir -p ~/Library/Logs/flaco ~/Documents/flaco-backups && \
  launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.roura.flaco.backup.plist && \
  launchctl kickstart -k gui/$(id -u)/io.roura.flaco.backup && sleep 2 && \
  ls -la ~/Documents/flaco-backups/'
```
Verify: most recent entry is from within the last minute.
Commit A2.

### STEP A3 — launchd supervisor
Write `deploy/io.roura.flaco.plist`. Stop the nohup process. Seed a default `/opt/homebrew/etc/flaco/config.toml`. Install and bootstrap the plist.
```bash
ssh mac-server 'launchctl print gui/$(id -u)/io.roura.flaco | grep -E "state|pid"'
# kill test:
ssh mac-server 'PID=$(launchctl print gui/$(id -u)/io.roura.flaco | awk "/pid/{print \$3; exit}"); kill $PID; sleep 12; curl -sS http://localhost:3033/health'
```
Commit A3.

### STEP B4 — flaco doctor
Add `Doctor` variant to `Command` enum, implement `doctor(cfg: &Config)` with 8 checks. Colored output. Non-zero exit on red.
```bash
cargo build --release -p flaco-v2 && scp rust/target/release/flaco-v2 mac-server:~/infra/flaco-v2
ssh mac-server 'codesign -fs - ~/infra/flaco-v2 && ~/infra/flaco-v2 doctor'
ssh mac-server 'FLACO_OLLAMA_URL=http://127.0.0.2:11434 ~/infra/flaco-v2 doctor; echo exit=$?'
```
Commit B4.

### STEP B1 — tool tier system
Create `rust/crates/flaco-core/src/tools/manifest.rs` (`Tier` enum + `ToolManifest`). Extend `Tool` trait. Register per-tool manifests. Add `ToolRegistry::effective_for(tier)` filter. Add `GET /tools` handler in `flaco-web`.
Verify: new integration test + live curl of `/tools?tier=default` vs `?tier=chris`.
Commit B1.

### STEP B2 — install-home.sh
Write `deploy/install-home.sh` with `--dry-run` support. Idempotent. Ends with `flaco-v2 doctor` call.
Verify: dry run enumerates ops; real run exits 0; second run also exits 0.
Commit B2.

### STEP B3 — install.sh for OSS
Write `deploy/install.sh`. Detects OS, checks Ollama, writes `tier=default` config, no credentials required.
Verify: `docker run --rm -v $PWD:/flaco ubuntu:22.04 bash /flaco/deploy/install.sh --non-interactive`.
Commit B3.

### STEP C1 — rubric review gate
**HALT.** DM Chris: "gate 1 of hackathon 3 — read `train/rubric.md` (R1-R11), reply approved or fix Rx". Wait. On approval, remove the `## Review checklist for Chris` section. Commit `rubric: freeze after Chris approval`.

### STEP C2 — seed interview gate
Write `train/data/seeds/interview_prompts.md` (40 prompts across R1-R11 + Chris voice + Walter voice). **HALT.** DM Chris the prompts. Wait for `interview_answers.md`. Parse into `train/data/seeds/seed_pairs.jsonl`. Commit.

### STEP C3 — full synthesis
```bash
OLLAMA_URL=http://mac.home:11434 python3 train/scripts/synthesize_pairs.py --variations-per-seed 50
python3 train/scripts/build_dataset.py
wc -l train/data/pairs/train.merged.jsonl
cat train/data/pairs/train.stats.json | jq ._total_dropped
```
If drop rate > 20%: set temperature 0.3, re-run. Commit.

### STEP C4 — Colab training
Upload `train/data/pairs/train.merged.jsonl` + open `train/colab/flaco_train.ipynb` in Colab. Runtime → T4 GPU. Run all. Download `flaco-custom-lora.tar.gz`. Ship to mac-server:
```bash
scp ~/Downloads/flaco-custom-lora.tar.gz mac-server:~/infra/
ssh mac-server 'cd ~/infra && tar -xzf flaco-custom-lora.tar.gz && cd flaco-custom && ollama create flaco-custom:7b -f Modelfile'
ssh mac-server 'ollama list | grep flaco-custom:7b'
```

### STEP C5 — router + eval regression
Edit `flaco-core::runtime::handle_turn` to pick a model based on keyword regex, calling the model the config says `[models] swift` maps to.
```bash
python3 train/scripts/eval_model.py --model qwen3:32b-q8_0 --model flaco-custom:7b --out /tmp/eval.json
curl -sS -X POST http://mac.home:3033/chat --data-urlencode 'message=Write a SwiftUI view with @Entry injection' | grep -q '@Entry'
```
Commit.

### STEP B5 — GRADING3.md
Write it with 10+ `##` sections and 25+ executable commands.
Verify: `grep -c '^##' GRADING3.md` and `grep -cE 'curl|ssh|cargo' GRADING3.md`.
Commit.

### STEP B6 — Slack notifications
Send DM to `elGordoRoura` via Slack `chat.postMessage`. Send `#infra-status` post. Log the timestamps.

### STEP Z — final smoke
```bash
bash deploy/smoke-v3.sh
```
Exit 0 == v3 shipped.

**P2 steps (D1-D5)** run after STEP Z, each as a standalone ≤30 min patch + commit.

## 12. Why This Plan Is Optimal

Data integrity (A1, done) and durability (A2/A3 backups + supervisor) come before features because the grader explicitly made those P0 and because a feature built on top of a broken substrate is a negative commit. A4 before A3 is non-negotiable because the launchd plist references the config path — building A3 first would mean editing the plist twice. B1 before B2 because `install-home.sh` writes a config key (`[tools] tier = "chris"`) that only exists once B1 is shipped. Training (C1-C5) runs in parallel with engineering once A4 is done, because Colab compute is the only time cost and doesn't block any other track. The alternative — training first, blockers last — leaves the system unreliable during the compute window, and any crash during that window would cost the un-backed-up delta. Every other ordering I considered creates rework or a window of fragility that this one does not.

## 13. Call to Action

Ready to start on **A4 (flaco-config crate)** — say "go" or redirect.
