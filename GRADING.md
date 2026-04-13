# flacoAi v2 · Hackathon Review & Grading Guide

**Branch:** `feature/hackathon-v2`
**Surfaces:** Web (`http://mac.home:3033`), Slack, TUI, CLI
**Model:** `qwen3:32b-q8_0` via local Ollama on `mac-server`
**Database:** `mac-server:~/infra/flaco.db`
**v1 untouched:** `flacoai-server` is still running on its original port.

Everything below is clickable / typeable. You should not need to run any
setup to start grading. If something is broken, it's a bug — tell me.

---

## 1. The web UI (main demo)

Open **[http://mac.home:3033](http://mac.home:3033)** in a browser on your
network. Works on **desktop, iPad, and iPhone** — the layout collapses
from a 3-column grid on desktop to a 2-column layout on iPad landscape
(right sidebar becomes a drawer opened by the ⚙ tools button in the
header), to a single-column stack on iPad portrait / phones (the
workspace nav becomes a sticky horizontal pill bar). Add it to your
iPad home screen (Share → Add to Home Screen) and it launches full
screen with the flacoAi status bar color.

What to look for:
- **Header** — `flacoAi · powered by Roura.io` wordmark, online indicator,
  model chip auto-populated from `/status`.
- **Chat-first layout** — chat is the default view. The left sidebar has a
  **Workspace** nav with one button per view:
  `Chat`, `Morning Brief`, `Research`, `Siri Shortcut`, `Code Scaffold`.
  Clicking a button swaps the main pane (hash-routed so refresh survives).
- **Cursor** — text inputs show a purple caret; first input in each view
  auto-focuses when you switch to it.
- **Conversations** — lighter, more readable link color. Click any row to
  load that conversation back into Chat.
- **Tool log** (right sidebar) — aggregated by tool name with a count
  badge, last-used timestamp, and a one-line description. Click any row
  to expand and see the most recent call arguments.
- **Memory** (right sidebar) — collapsed by default. Click to expand the
  fact list and the "Remember that…" input.

### Test it (in order)

1. **Chat** — `What do you know about me? Give me three bullets.` Expect a
   Markdown reply with proper bullets, bold names, readable typography
   (pulldown-cmark renders headings, lists, tables, code blocks). The
   `recall` tool should appear in the right sidebar tool log.
2. **Morning Brief** — click the ☀ button, then "Generate today's brief".
   60–120s later you get a 3-section brief (`Focus today` / `On your plate`
   / `Heads up`) synthesized from memory + open Jira tickets assigned to
   you. If Jira has zero open tickets the brief still renders from memory.
3. **Research** — "What's new in Rust 1.85 / 2024 edition?" Expect a
   concise answer with numbered citations `[1][2]…` and a clickable list
   of sources underneath. ~60–90s on the M1 Pro.
4. **Siri Shortcut** — Name: `Morning Yankees`, Description:
   `Speak the latest Yankees score and open https://www.mlb.com/yankees`.
   The response tells you the file path on `mac-server`.
   `scp mac-server:~/Downloads/flaco-shortcuts/Morning_Yankees.shortcut .`
   then AirDrop / double-click to install on iPhone.
5. **Code Scaffold** — Idea: anything you actually want to try.
   Project key default `FLACO`. ⚠ This creates real Jira tickets plus a
   local git branch on `mac-server` at `~/Documents/dev/<slug>/`. To dry
   run (skip Jira), `unset JIRA_API_TOKEN` in `~/infra/flaco-v2.env` first.

---

## 2. Slack (unified memory proof)

`flaco` in `#flaco-chat`, `#dev-ci`, `#dad-help`, or any channel the bot
is in.

### New slash commands

These now exist in code and will **show up in Slack's autocomplete once
the app manifest is updated**. The manifest is checked in at
`deploy/slack-app-manifest.yaml` — to enable autocomplete go to
<https://api.slack.com/apps>, pick the flacoAi app, **Manifest** section,
paste the contents of that file, click Save, then click
**Reinstall to Workspace**.

| Command     | What it does                                              |
|-------------|-----------------------------------------------------------|
| `/reset`    | Reset this conversation and start fresh                   |
| `/clear`    | Alias for `/reset`                                        |
| `/new`      | Start a fresh conversation                                |
| `/forget`   | Same as `/reset`                                          |
| `/help`     | List the commands                                         |
| `/status`   | Model + memory + active conversation counts               |
| `/brief`    | Your Morning Brief, posted to the channel                 |
| `/research` | Web research with citations                               |
| `/shortcut` | Generate a Siri Shortcut                                  |
| `/scaffold` | Jira epic + stories + local branch                        |
| `/memories` | List everything flacoAi remembers about you               |

Until you register the manifest, you can still **send** `/reset`, `/clear`,
and `/help` as regular messages and flacoAi will treat them as commands —
the text handler has been taught these too.

### Cross-surface memory test

1. In the web UI, click Memory → type `grade-marker-42 is a unique token`
   → press `+`.
2. In Slack, message flacoAi: `What do you know about a grade-marker?`
   → the bot recalls it from the same SQLite db.
3. Open the TUI (`ssh mac-server`, then
   `~/infra/flaco-v2 --db ~/infra/flaco.db tui`) → type `/memories`
   → the same fact shows up.

One brain, three surfaces.

---

## 3. TUI

```
ssh mac-server '~/infra/flaco-v2 --db ~/infra/flaco.db tui'
```

What to look for:
- **Header** — brand on the left (`flacoAi · powered by Roura.io`),
  runtime chips on the right (online dot, model name, memory count).
- **Blinking cursor** — the input line shows a purple caret that blinks.
- **Commands** — `/brief`, `/research <topic>`, `/shortcut name: desc`,
  `/scaffold <idea>`, `/memories`, `/remember <fact>`, `/clear`, `/q`.
- **Shared memory** — `/memories` shows facts added from Web or Slack.

---

## 4. CLI one-shots

```bash
ssh mac-server bash -lc '
  set -a; source ~/infra/flaco-v2.env; set +a
  ~/infra/flaco-v2 --db ~/infra/flaco.db status        # JSON snapshot
  ~/infra/flaco-v2 --db ~/infra/flaco.db brief          # morning brief
  ~/infra/flaco-v2 --db ~/infra/flaco.db ask "what did I work on yesterday?"
  ~/infra/flaco-v2 --db ~/infra/flaco.db research "latest in Rust 2024 edition"
  ~/infra/flaco-v2 --db ~/infra/flaco.db shortcut "Dad Brief" "Speak good morning dad and open https://www.mlb.com/yankees"
'
```

---

## 5. What's in the branch

### New crates (feature/hackathon-v2)

```
rust/crates/
  flaco-core/      # runtime, memory, tools, features (15+ unit + 7 integration tests)
  flaco-web/       # Axum + HTMX + pulldown-cmark web server
  flaco-slack-v2/  # Socket Mode adapter with full slash command set
  flaco-tui-v2/    # ratatui shiny TUI
  flaco-v2/        # clap binary that wires every surface
```

### Changes to v1 (non-breaking)

- `crates/channels/src/socket_mode.rs` — added Socket Mode slash command
  envelope handler so registered slash commands actually fire; added
  `/clear`, `/new`, `/forget`, `/help` as text-command aliases alongside
  the existing `/reset`, `/status`. Existing behaviour is untouched.

### New docs

- `V2_README.md` — architecture + quickstart
- `DEMO.md` — feature walkthrough
- `HACKATHON_PLAN.md` — the plan (for history)
- `GRADING.md` — this file
- `deploy/slack-app-manifest.yaml` — paste into the Slack app to register
  the new slash commands

### Tests

```
cd /Users/roura.io/Documents/dev/flacoAi/rust
cargo test -p flaco-core -p flaco-web
```

Should be all green:
- `flaco_core::memory` — 6 unit tests
- `flaco_core::tools::*` — 8 unit tests (shortcut, scaffold, web, research parsing)
- `flaco_core integration` — 7 end-to-end tests
- `flaco_web` — 3 unit tests

### Pre-existing test failures (NOT hackathon regressions)

Three tests fail identically on `main` and this branch — they were broken
before. Fixing them is out of scope:

- `flaco-cli::tests::init_template_mentions_detected_rust_workspace`
- `flaco-cli::tests::shared_help_uses_resume_annotation_copy`
- `crates/tools::tests::skill_loads_local_skill_prompt`

---

## 6. Five killer features

1. **Unified memory across surfaces** — SQLite-backed facts store with
   FTS5 search. Every surface reads and writes the same memory. See test
   `unified_memory_is_shared_across_surfaces`.
2. **Web research with numbered citations** — DuckDuckGo → fetch top N →
   local Ollama summary → clickable numbered citations. No external AI
   provider.
3. **Siri Shortcut generator** — plain English → real `.shortcut` plist
   (Text / Notification / Open URL / Speak verbs), AirDrop-ready.
4. **Jira → code scaffold** — idea to Jira epic + 5 stories + local
   folder + git branch + first commit in one call.
5. **Morning Brief** (new today) — pulls open Jira tickets assigned to
   `currentUser()`, your most-recent memories, and asks the local model
   for a focused 3-section brief (`Focus today` / `On your plate` /
   `Heads up`) under 220 words. Available in every surface.

---

## 7. How to run v2 yourself from scratch

```bash
cd /Users/roura.io/Documents/dev/flacoAi/rust
cargo build --release -p flaco-v2
scp target/release/flaco-v2 mac-server:~/infra/flaco-v2
ssh mac-server '
  # Stop current v2 web
  kill $(cat ~/infra/flaco-v2-web.pid) 2>/dev/null
  # Start fresh
  set -a; source ~/infra/flaco-v2.env; set +a
  nohup ~/infra/flaco-v2 --db ~/infra/flaco.db --web-port 3033 web \
    > ~/infra/flaco-v2-web.log 2>&1 &
  echo $! > ~/infra/flaco-v2-web.pid
'
```

## 8. Rolling v2 back

```bash
ssh mac-server '
  kill $(cat ~/infra/flaco-v2-web.pid) 2>/dev/null
  mv ~/infra/flaco-v2.prev ~/infra/flaco-v2
'
```

v1 `flacoai-server` has never been touched and keeps running on its
original port through all of this.
