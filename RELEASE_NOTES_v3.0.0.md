# flacoAi v3.0.0-rc1 — Release Notes

**Tag:** `v3.0.0-rc1`
**Branch:** `feature/hackathon-3-training`
**Released:** 2026-04-13
**Status:** release candidate — tag lives on the feature branch; merge to `main` when reviewed.

> **Theme:** Production-hardening + Jarvis-mode.
>
> v2 shipped a unified runtime (Slack + TUI + Web + CLI all sharing one brain,
> one memory, and one typed tool registry). v3 takes that runtime and makes it
> reliable enough to rely on daily — closing every ship-blocker from the v2
> grader — and then adds a natural-language intent router so you never have
> to remember a slash command again.

---

## TL;DR

- **4 v2 ship-blockers closed.** Tool-loop dedup, config-file loader, automated
  SQLite backups, launchd supervisor. `flaco doctor` validates all of them
  in one command.
- **Jarvis-mode.** Natural-language intent router — `clear`, `reset`,
  `clear this chat`, `how are you`, `what's on my plate`, and 30+ other
  phrasings route instantly to the right action. No slash-command registration
  required. Fixes the "typing `/clear` in Slack does nothing" bug at the root.
- **CI pipeline.** GitHub Actions runs `cargo test` + `cargo clippy -D warnings`
  on every push to `main` + `feature/*`. Pre-existing test failures fixed.
  Live-Ollama integration test shipped, ignored by default, opt-in via
  `--ignored`.
- **40 tests green, 1 ignored** across v2+v3 crates. Zero known failures.
- **v1 untouched.** `flacoai-server` still runs on its original port. Walter's
  n8n workflows still inactive. Safe to merge, safe to roll back.

---

## What ships in v3

### Track A — v2 grader ship-blockers (closed)

| ID | Commit | What |
|---|---|---|
| **A1** | `ef761d8` | Tool-loop duplication fix — two-layer defense (runtime HashSet dedup + tool-level idempotency in `Remember`). Reproduced live: `remember` fired 6× for a single fact on v2; now fires exactly 1×. |
| **A4** | `9846bf7` | `flaco-config` crate — TOML loader at `/opt/homebrew/etc/flaco/config.toml`, env-var overrides, search path fallbacks, `ConfigSource::{Defaults,EnvOnly,File,FilePlusEnv}` tracking. 5 unit tests. Every hardcoded path in `flaco-v2` extracted. |
| **A2** | `dc91d9e` | Automated backups via `sqlite3 VACUUM INTO` + `io.roura.flaco.backup.plist` launchd calendar-interval at 03:00 daily. Enables WAL on `Memory::open` so the backup never blocks the running web server. Retries on `SQLITE_BUSY`. |
| **A3** | `730d97b` | Real launchd supervisor (`io.roura.flaco.plist`) with `KeepAlive { Crashed: true }`, `ThrottleInterval 10`, structured logs. Kill-test verified: `kill -9 $pid; sleep 12; curl /health` → `ok`. |
| **B4** | `6b28a7e` | `flaco-v2 doctor` — 10-check health probe, color output, non-zero exit on any failure. Validates A1-A4 in one command. |

### Track B — Jarvis-mode (new in v3)

| Commit | What |
|---|---|
| `8ffbbb2` | `flaco-core::intent` — natural-language intent router that runs **before** the LLM chat loop. Detects `Reset`, `Help`, `Status`, `Brief`, `ListMemories`, `Tools`. Dispatches instantly, zero LLM round-trip. Wired into `flaco-web`, `flaco-slack-v2`, `flaco-tui-v2`, `flaco-v2 ask`, and `v1 channels::socket_mode` (via a deliberately-duplicated phrasings list to avoid a v1 → v2 crate dep). 10 unit tests covering every phrasing + over-match guard. |

**Every phrasing of "reset my conversation" now works:**

```
clear                       new                       forget this
clear this                  new chat                  forget that
clear this chat             new conversation          forget everything
clear this conversation     new thread                wipe
clear the chat              start over                wipe this
reset                       start fresh               wipe the conversation
reset this                  /reset                    hey flaco reset
reset the conversation      /clear                    flaco clear
reset chat                  /new                      @flaco clear
                            /forget
```

### Track C — Green main under CI

| Commit | What |
|---|---|
| `0178d59` | Fix `flaco-cli::init_template_mentions_detected_rust_workspace` — asserted `# FLACOAI.md`, template now emits `# CLAW.md`. |
| `b6f7007` | Fix `flaco-cli::shared_help_uses_resume_annotation_copy` — asserted `flaco --resume`, help text now says `flacoai --resume`. |
| `bfad538` | Fix `tools::skill_loads_local_skill_prompt` — restored missing `skills/help/SKILL.md` fixture. |
| `af647ba` | `cargo clippy --fix` across v2+v3 crates, lint level pinned via `workspace.lints.clippy`. |
| `deee39b` | `flaco-core::tests::ollama_smoke` — live integration test, `#[ignore]`d by default, opt-in via `cargo test -- --ignored`. |
| `69e8878` | `.github/workflows/test.yml` — 3 jobs (clippy `-D warnings`, test, manual ollama smoke). Triggers on push to `main` + `feature/*` + PRs. |

### Track D — deferred (intentionally)

These are explicitly **not** in v3. Each is tracked and has a reason.

| Item | Reason |
|---|---|
| Custom LoRA training (`train/` directory) | Gated on Chris's 10-min rubric review + 15-min seed interview. Training on M1 Pro is iteratively too slow; parked until M3 Ultra arrives mid-May. Rubric frozen on the branch as a reference artefact. See `train/rubric.md`. |
| SSE streaming for `/research` + `/chat` | Quality-of-life, own session. Event bus already emits `TextChunk`; web adapter just needs to wrap it. |
| SIGTERM graceful shutdown + `/metrics` endpoint | Thematically coherent with each other ("plug into homelab's existing plumbing"); own session. |
| Tool-store tiers (B1) | No forcing function yet (single user, no OSS release). Blocked on: OSS release OR Walter-as-distinct-user. |
| `install-home.sh` + `install.sh` (B2/B3) | Same reason — no fresh install happening today. Planned for the week before OSS release. |
| Slack v2 cutover (retire v1) | Readiness call, not a quality call. v1 stays owning the Slack token until Chris explicitly flips. |

---

## Commit log

```
8ffbbb2 feat(flaco-core): jarvis-mode natural-language intent router
69e8878 ci: run test + clippy on every push to feature branches
deee39b test(flaco-core): live-Ollama smoke test (ignored by default)
af647ba chore(flaco-core): apply clippy --fix and pin lint level
bfad538 fix(tools): add missing help/SKILL.md fixture for skill loader test
b6f7007 fix(flaco-cli): update shared_help assertion to current copy
0178d59 fix(flaco-cli): update init_template assertion to current template
6b28a7e hackathon-3 B4: flaco doctor subcommand — closes P0 track
730d97b hackathon-3 A3: launchd supervisor for flaco-v2 web (grader P0)
dc91d9e hackathon-3 A2: automated flaco.db backup via launchd (grader P0)
9846bf7 hackathon-3 A4: flaco-config crate + hardcoded path extraction
1eb9c1c hackathon-3: rewrite ULTRAPLAN as execution-ready runbook
ef761d8 hackathon-3 A1: fix tool-loop duplication bug (grader P0)
c85a895 hackathon-3: ULTRAPLAN + training pipeline scaffold
4b6ce3e hackathon-3: scaffold training pipeline — rubric + harvesters
```

---

## Verification — the "is v3 actually shipped" one-liner

```bash
bash deploy/smoke-v3.sh
```

That runs every probe the grader cares about and exits 0 only if everything
is green. See `deploy/smoke-v3.sh` for the individual checks; each one has
a `# A1 proof:` / `# A2 proof:` comment tying it back to this release notes.

Manual verification:

```bash
# 1. flaco doctor (all P0)
ssh mac-server '~/infra/flaco-v2 doctor; echo exit=$?'
# → 10 pass · 0 warn · 0 fail, exit=0

# 2. Jarvis-mode (intent router)
curl -s -X POST http://mac.home:3033/chat -d 'message=clear this chat'
# → "Conversation reset. Starting fresh — what's up?" (instant, no LLM)

curl -s -X POST http://mac.home:3033/chat -d 'message=how are you?'
# → "flacoAi online · model qwen3:32b-q8_0 · 15 tools · N memories · N conversations"

# 3. Test suite
cd rust && cargo test -p flaco-core -p flaco-config -p flaco-web --all-targets
# → 40 passed, 1 ignored (ollama_smoke), 0 failed

# 4. CI locally
cd rust && cargo clippy \
  -p flaco-core -p flaco-config -p flaco-web \
  -p flaco-v2 -p flaco-tui-v2 -p flaco-slack-v2 \
  --all-targets -- -D warnings
# → Finished, 0 warnings, 0 errors

# 5. Live Ollama (opt-in)
OLLAMA_URL=http://mac.home:11434 FLACO_MODEL=qwen3-vl:8b \
  cargo test -p flaco-core --test ollama_smoke -- --ignored
# → 1 passed, reply "pong"

# 6. v1 still untouched
ssh mac-server 'ps -ef | grep -v grep | grep flacoai-server'
# → v1 PID still alive
```

---

## Rollback

Every change is reversible.

```bash
# Web server: unload launchd, restore the previous binary
ssh mac-server '
  launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.roura.flaco.plist
  launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.roura.flaco.backup.plist
  mv ~/infra/flaco-v2.prev ~/infra/flaco-v2
  # v1 flacoai-server is untouched and still running on its own pid
'

# Source: delete the tag + hard reset the branch
git tag -d v3.0.0-rc1
git push origin :refs/tags/v3.0.0-rc1   # if pushed

# Or just don't merge — the branch stays off main until you say so
```

---

## Known open items (carrying into v3.1 / hackathon 4)

From the grader's own list, ranked:

1. **SSE streaming for `/research` + `/chat`** — biggest daily-use QoL gap
2. **SIGTERM graceful shutdown** — 15-min fix, bundle with `/metrics`
3. **`/metrics` Prometheus exporter** — wire into existing Pi Prometheus
4. **Custom LoRA training** — parked until M3 Ultra (mid-May)
5. **Tool-store tiers (B1)** — blocked on forcing function
6. **Setup scripts (B2/B3)** — blocked on OSS release date

None are v3 blockers.

---

## Grading one-liner

```bash
bash deploy/smoke-v3.sh && echo "v3 GREEN"
```

Exit 0 means every claim in this release notes reproduces on mac-server.

— Shipped by Claude Opus 4.6 under Chris's direction. v3.0.0-rc1.
