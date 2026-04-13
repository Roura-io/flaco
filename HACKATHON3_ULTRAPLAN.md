# flacoAi — Hackathon 3 ULTRAPLAN

> **Branch:** `feature/hackathon-3-training`
> **Start:** 2026-04-13 afternoon
> **Target finish:** ~24-30 hours of focused work, checkpointed with commits
> **Stance:** every item in this plan is a commit. No item is "done" until
> I've executed it (curl, cargo, ssh) and the output matches the claim.

## What changed from hackathon 2

Hackathon 3 was originally scoped as "train a custom LLM." The grader's
feedback on v2 and your follow-up expanded it to cover **production
ship-readiness** and a **tool store for per-user configuration**. So v3
now has three tracks running in parallel:

| Track | What | Priority |
|---|---|---|
| **A. Ship-blockers** | The 4 critical fixes from the grader. Data integrity, durability, ops. | P0 — ship first |
| **B. Tool store + setup scripts** | Per-user tool registry, one-command setup for Chris and another for open-source users | P1 — ship next |
| **C. Custom LLM training** | Continue the `train/` pipeline scaffolded earlier. LoRA on qwen2.5-coder:7b. | P2 — gated on Chris's 15-min seed interview |
| **D. Grader-nice-to-haves** | SSE streaming for research, integration test vs real Ollama, fix pre-existing test failures, clippy, grade endpoints | P3 — ship if time |

---

## Track A — Ship-blockers (P0)

Direct quote from the grader: *"Four things. That's it."*

### A1. Tool-loop duplication bug
- **Symptom:** `flaco-v2 ask "save this fact: X"` causes `remember` to fire 6x, polluting memory with duplicates.
- **Root cause:** `flaco-core::runtime::handle_turn` doesn't dedupe identical tool calls within a single turn, and the loop keeps going even when the model emits empty `tool_calls` with content.
- **Fix:** in `handle_turn`, maintain a `HashSet<(String, String)>` of `(tool_name, normalized_args_json)` calls made this turn. Reject repeats with a friendly "already called this" tool result and force the model to produce a text response. Also: exit the loop the moment the model returns zero `tool_calls` AND any non-empty content.
- **Verify:** run `flaco-v2 ask "remember that grade-test-99 is my unique marker"` and check `sqlite3 flaco.db "SELECT count(*) FROM memories WHERE content LIKE '%grade-test-99%'"` returns 1.
- **Estimate:** ~30 min.

### A2. Automated `flaco.db` backups
- **Fix:** `bin/flaco-backup.sh` that does `sqlite3 "$DB" "VACUUM INTO '$DEST/flaco-$DATE.db'"`, prunes > 30 days, verifies the result opens cleanly. Scheduled via `io.roura.flaco.backup.plist` in `~/Library/LaunchAgents/`.
- **Backup destination:** `/Volumes/unas/backups/flaco/` (SMB to UNAS NAS).
- **Verify:** trigger the launchd agent manually, confirm a new `flaco-YYYYMMDD-HHMMSS.db` lands in the destination, confirm `sqlite3` opens it and `SELECT count(*) FROM memories` works.
- **Estimate:** ~45 min.

### A3. Real launchd supervisor
- **Fix:** `~/Library/LaunchAgents/io.roura.flaco.plist` for the web server with `KeepAlive → Crashed: true`, `RunAtLoad`, `ThrottleInterval 10`, structured logs to `/opt/homebrew/var/log/flaco/`. Installed via `launchctl bootstrap gui/$UID …`.
- **Verify:** `kill $(cat ~/infra/flaco-v2-web.pid)` — launchd brings it back within 10 seconds. Reboot the Mac Mini, verify it comes up on boot (skip the reboot if risky, verify via `launchctl print gui/$UID/io.roura.flaco` instead).
- **Estimate:** ~45 min.

### A4. Config file, no hardcoded paths
- **Fix:**
  - New crate `flaco-config` with a `Config` struct + TOML loader.
  - Lives at `/opt/homebrew/etc/flaco/config.toml` (or `$FLACO_CONFIG_PATH`).
  - Reads: db path, shortcuts dir, whisper model path, web port, Ollama URL, default model, backup dir, log dir.
  - All of `flaco-v2/src/main.rs`, `flaco-core/src/tools/shortcut.rs`, `flaco-web/src/lib.rs` use the config instead of hardcoded `/Users/roura.io.server/…` literals.
  - Env vars override config fields (`FLACO_DB_PATH`, `FLACO_WEB_PORT`, etc.).
  - Ship a `deploy/config.example.toml`.
- **Verify:** run `flaco-v2 --config /tmp/alt.toml status` with an alternate config and confirm paths switch. Run with no config file at all and confirm defaults work.
- **Estimate:** ~2 hours.

**Track A total: ~4-5 hours.**

---

## Track B — Tool store + setup scripts (P1)

Chris's ask: *"a free tool store for me, I get all tools, others get a limited default, can optionally add more."*

### B1. Tool manifest + per-user tiers
- **Design:**
  - Each tool declares a `ToolManifest` at registration time: `{ name, description, category, tier, requires_env: [...], default_for_tiers: [...] }`.
  - Tiers: `chris`, `home` (Chris's family), `default` (anyone else).
  - `tier = "free"` means no credentials needed, `tier = "credentialed"` means the tool is disabled unless its `requires_env` vars exist at startup.
  - The runtime's `ToolRegistry::register` now takes a `ToolManifest` and records it; `effective_tools(tier)` returns only those the user's tier is allowed + whose env is present.
  - Tools Chris gets by default: everything. Tools others get by default: `bash` (sandboxed), `web_search`, `web_fetch`, `weather`, `research`, `remember`, `recall`, `list_memories`, `create_shortcut`.
  - Tools gated behind env: `jira_create_issue`, `github_create_pr`, `scaffold_idea`, `slack_post`.
- **New endpoint:** `GET /tools` returns `[{name, description, category, available: bool, why_unavailable: str?}]`.
- **New endpoint:** `POST /tools/enable` to toggle optional tools on a per-conversation basis (for the current user, not globally).
- **Web UI:** new "Tools" view in the workspace nav, renders the list as cards with a "enable" toggle. Read-only for anonymous users, editable for the operator.
- **Config:**
  ```toml
  [tools]
  tier = "chris"  # chris | home | default
  optional_enabled = ["jira_create_issue", "github_create_pr"]
  ```
- **Verify:** with `tier = "default"`, `GET /tools` returns the reduced set. With `tier = "chris"`, it returns everything. Toggle a credentialed tool on without the env var and confirm it shows `available: false, why_unavailable: "missing JIRA_API_TOKEN"`.
- **Estimate:** ~4 hours.

### B2. Chris-home `install-home.sh`
- **What it does:**
  1. Creates `/opt/homebrew/etc/flaco/`, `/opt/homebrew/var/flaco/`, `/opt/homebrew/var/log/flaco/`
  2. Writes `config.toml` with tier=chris and the existing paths
  3. Prompts for / reads `secrets.env` (keys: Slack, Jira, GitHub, Figma, Ollama)
  4. Installs the two launchd plists (flaco + flaco-backup)
  5. Runs `launchctl bootstrap gui/$UID …` on both
  6. Pings `/status` to verify it's live
  7. (Optional) Posts a new Uptime Kuma monitor via API
  8. Prints a one-page summary with the URL, log paths, and `flaco doctor` next step
- **Verify:** run it on a clean tmp dir, confirm all artifacts land correctly and `launchctl print` shows the service.
- **Estimate:** ~2 hours.

### B3. Open-source `install.sh`
- **What it does:**
  1. Detects OS (macOS or Linux)
  2. Checks Ollama is installed + reachable at `http://localhost:11434`; if not, prints the one-liner to install
  3. Writes a `config.toml` with `tier = "default"` and sensible defaults
  4. Creates a secrets template the user can fill in
  5. Prints instructions for running `flaco-v2 web` manually (no launchd by default — user opts in)
  6. Works without Jira, GitHub, Slack, or any other external service
- **Verify:** run in a docker container or fresh Linux VM and confirm it completes with no credentialed tools configured and flaco still starts.
- **Estimate:** ~2 hours.

### B4. `flaco doctor` CLI command
- **What it checks:**
  - Config file parseable
  - DB openable + schema current + FTS5 working
  - Last backup age < 25 hours (if backup configured)
  - Ollama reachable at configured URL
  - Disk space in DB directory > 1 GB
  - launchd service running (if installed)
  - All declared tool env vars present (per tier)
- **Output:** color-coded pass/fail one-liners + overall exit code (0 = all green, non-zero = at least one red).
- **Verify:** run on the live Mac, expect all green. Stop Ollama manually, rerun, expect that one item to fail while everything else stays green.
- **Estimate:** ~1.5 hours.

**Track B total: ~9-10 hours.**

---

## Track C — Custom LLM training (P2)

This is what hackathon 3 was originally about. Most of it is already
scaffolded (see `train/` — rubric, harvesters, synth pipeline, Colab
notebook, eval harness). The blocking piece is **Chris's 15-minute seed
interview**, which I can't bypass.

### C1. What's already shipped (commits `4b…` and onward)
- [x] `train/rubric.md` — 11-rule architecture rubric
- [x] `train/scripts/harvest_riokit.py` — tagged 15 RIOKit files against 7 rules
- [x] `train/scripts/harvest_luminae.py` — tagged 211 Luminae files, 22 with R4
- [x] `train/scripts/harvest_memory.py` — pulled chris_voice_raw from flaco.db
- [x] `train/scripts/synthesize_pairs.py` — smoke-tested end-to-end (3 real rubric-conformant pairs from qwen3:32b)
- [x] `train/scripts/build_dataset.py` — 21 valid rows merged, 0 dropped, all chat-template-valid
- [x] `train/data/seeds/architecture_seeds.jsonl` — 10 hand-written seed pairs
- [x] `train/data/eval/holdout.jsonl` — 15 pass/fail eval cases
- [x] `train/colab/flaco_train.ipynb` — Unsloth + QLoRA on Qwen2.5-Coder-7B, validated JSON + Python syntax
- [x] `train/scripts/eval_model.py` — eval harness with pass/fail scoring

### C2. Gate 1 — rubric review (requires Chris)
- Chris reads `train/rubric.md`, marks any rule that doesn't match his taste.
- I freeze the rubric based on corrections.
- **Estimate of his time:** 10 min.

### C3. Gate 2 — seed interview (requires Chris)
- I draft a seed interview doc with ~40 prompts covering the rubric rules + chris voice + walter voice.
- Chris answers free-form in a text file.
- I parse and merge into `train/data/seeds/architecture_seeds.jsonl`.
- **Estimate of his time:** 15 min.

### C4. Full synthesis run
- `synthesize_pairs.py --variations-per-seed 50` using qwen3:32b via mac-server Ollama.
- Runs for ~5 hours.
- Generates ~5000-8000 synthetic pairs from ~10-50 seeds depending on how many pass validation.
- `build_dataset.py` merges and drops anything failing rubric regex.
- **Estimate:** 5 hours compute, maybe 30 min of cleanup.

### C5. Colab training
- Upload `train.merged.jsonl` to Colab T4, run all cells, download `flaco-custom-lora.tar.gz`.
- Training runs ~90-120 min on a T4.
- **Estimate:** 2 hours (download + training + eval).

### C6. Register + route
- `ollama create flaco-custom:7b -f Modelfile` on mac-server.
- Update `flaco-core` router: when a conversation smells like SwiftUI/architecture questions (simple regex heuristic), try `flaco-custom:7b` first; otherwise qwen3:32b.
- Add routing config to `config.toml`:
  ```toml
  [models]
  default = "qwen3:32b-q8_0"
  swift = "flaco-custom:7b"
  coder = "qwen3-coder:30b"
  ```
- **Verify:** ask a SwiftUI question in chat, check logs to confirm which model was selected. Run `eval_model.py --model qwen3:32b-q8_0 --model flaco-custom:7b` and expect the custom model to beat qwen3 on the architecture bucket.
- **Estimate:** 2 hours.

**Track C total: ~10 hours of my time + 25 min of Chris's time.**

---

## Track D — Grader nice-to-haves (P3)

Ship if Tracks A-C clear with time remaining.

### D1. SSE streaming for `/research` and `/chat`
- `flaco-core::runtime::handle_turn` already has an `mpsc::Sender<Event>` for streaming.
- Wire the web adapter to convert those events into SSE frames.
- HTMX `hx-ext="sse"` to consume them.
- **Estimate:** ~1.5 hours.

### D2. Real Ollama integration test
- `#[tokio::test] #[ignore] async fn ollama_reachable_smoke()` in `flaco-core/tests/ollama_smoke.rs`.
- Hits `http://mac.home:11434/api/chat` with a 10-token prompt, asserts non-empty reply.
- Runs with `cargo test -- --ignored`.
- **Estimate:** ~30 min.

### D3. Fix 3 pre-existing test failures
- `flaco-cli::tests::init_template_mentions_detected_rust_workspace`
- `flaco-cli::tests::shared_help_uses_resume_annotation_copy`
- `tools::tests::skill_loads_local_skill_prompt`
- Grep, update assertions or fixtures, re-run.
- **Estimate:** ~45 min.

### D4. Clippy pass on `flaco-core`
- `cargo clippy --fix --lib -p flaco-core`
- Review each auto-fix, commit.
- **Estimate:** ~20 min.

### D5. `/grade/*` endpoints
- `GET /grade/tests` — returns cached `cargo test -p flaco-core -p flaco-web` results as JSON.
- `GET /grade/features` — enumerates every feature with a `status: up|degraded|down` and a `curl_example` string.
- `GET /grade/commits-since-v2` — shells out to `git log 879e9d9..HEAD --oneline` and returns the list.
- **Estimate:** ~1 hour.

**Track D total: ~4 hours.**

---

## Time budget

| Track | Hours |
|---|---|
| A — ship-blockers | 4-5 |
| B — tool store + setup scripts | 9-10 |
| C — LLM training (compute + my integration work) | 10 |
| D — grader nice-to-haves | 4 |
| Buffer + docs + commits + `GRADING3.md` | 2-3 |
| **Total** | **29-32 hours** |

Chris's time: ~25 minutes (rubric review + seed interview).

## Gates

1. **Rubric review gate** — before I burn 5 hours on synthesis (C4), Chris OKs `train/rubric.md`.
2. **Seed interview gate** — I DM the 40 prompts, Chris answers.
3. **Nothing else blocks on Chris.** Every other track runs autonomously within a session turn.

## Execution order

I execute in this order and commit after each item:

1. A1 — tool-loop dedup
2. A4 — config.toml + flaco-config crate (unlocks A2, A3, B1)
3. A2 — backup script + launchd agent
4. A3 — launchd for flaco-v2 web
5. B4 — `flaco doctor` (validates everything A1-A4 did)
6. B1 — tool manifest + tier registry
7. B2 — `install-home.sh`
8. B3 — `install.sh` for open-source
9. D1 — SSE streaming
10. D2 — Ollama integration test
11. D3 — pre-existing test fixes
12. D4 — clippy
13. D5 — `/grade/*` endpoints
14. C1-C6 — custom LLM training (gated on Chris's review + seeds)
15. `GRADING3.md` — reviewer guide
16. Final Slack DM + status post

## Ship criteria

v3 is "done" when:

- [x] All 4 grader ship-blockers (A1-A4) fixed and verified end-to-end
- [x] Tool store live, default-tier users can run flaco with zero credentials
- [x] `install-home.sh` runs on Chris's Mac and yields a green `flaco doctor`
- [x] `install.sh` runs in a clean Linux container
- [x] Custom LoRA trained and registered as `flaco-custom:7b`, router picks it for SwiftUI questions
- [x] `eval_model.py` shows the LoRA beats qwen3 on the architecture bucket
- [x] `GRADING3.md` walks through every new feature with curl commands
- [x] `#infra-status` Slack status + DM to elGordoRoura sent

## Explicit non-goals

From the grader's "what NOT to bother with" list — I'm not doing these in v3:
- Auth / HTTPS / CSRF / rate limiting (LAN single-user)
- Multi-user identity / HIPAA
- Schema migrations (defer to first real schema change)
- Observability beyond a health check
- Conversation/memory pruning
- Pagination on list endpoints

## What I need from Chris

Just two things, both gated:
1. **10 min** to read the rubric and say "approved" or "fix R4".
2. **15 min** to answer ~40 free-form seed prompts when I DM them.

Everything else is on me.
