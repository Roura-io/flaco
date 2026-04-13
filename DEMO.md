# flacoAi v2 — Demo Walkthrough

Open a browser to **http://mac.home:3033**. That's flaco v2 running on
`mac-server` beside the untouched v1.

Prefer the terminal? The same binary does TUI, one-shots, and slash
commands.

---

## 1. Unified memory across surfaces

The v2 db was pre-seeded at deploy time from markdown fact files on disk,
so flaco already knows who Chris is, what the RouraIO homelab looks like,
and Chris's feedback preferences.

### Try it

**Web:** type into the chat:

```
What do you know about me? Give me three bullets.
```

flaco will call `recall`, read the seeded facts, and summarize. Watch the
**Tool log** panel on the right — you'll see the `recall` call show up
within a second.

**CLI:**

```bash
~/infra/flaco-v2 --db ~/infra/flaco.db ask "What do you know about me?"
```

**TUI:**

```bash
~/infra/flaco-v2 --db ~/infra/flaco.db tui
> what do you know about me?
```

Then add a new memory in the sidebar: type `"Chris is a Yankees fan"` in
the right-hand Memory box, press `+`. Reload the TUI — the fact shows up
via `/memories`. The brain is the same on every surface.

---

## 2. Web research with citations

### Try it

**Web:** use the **research** form.

```
What's new in Rust 2024 edition?
```

flaco will DuckDuckGo search, fetch the top 4 pages, ask qwen3:32b to
write a ≤250-word answer constrained to only the sources, and return
numbered citations. Sources render as clickable links.

**CLI:**

```bash
~/infra/flaco-v2 --db ~/infra/flaco.db research "Latest macOS Tahoe release notes"
```

Expect ~60-90s on the M1 Pro (32B Q8). When the Mac Studio arrives this
drops to seconds.

---

## 3. Siri Shortcut generator

### Try it

**Web:** use the **siri shortcut generator** form.

```
Name:        Morning Yankees
Description: Speak the latest Yankees score and open https://www.mlb.com/yankees
```

The response tells you the .shortcut file was written to
`~/Downloads/flaco-shortcuts/Morning_Yankees.shortcut` on mac-server.
`scp` or AirDrop it to your iPhone, tap it, and it's live.

**CLI:**

```bash
~/infra/flaco-v2 shortcut "Dad Brief" "Speak good morning dad and open https://www.mlb.com/yankees"
```

The generator produces a real Apple WFWorkflow XML plist. Current verbs:
Text, Notification, Open URL, Speak. Adding more is a pattern-match on
the English description.

---

## 4. Jira → code scaffold

One command takes "I want to build X" to a ready-to-code branch:

1. Creates a Jira **Epic** under the given project key
2. Creates 5 **Stories / tasks** under that epic (spike → scaffold → core
   happy path → tests → polish)
3. Creates a local folder at `~/Documents/dev/<slug>/` with `README.md`,
   `CLAUDE.md`, `Cargo.toml`, `src/main.rs`
4. `git init` + new branch + first commit

### Try it

**Web:** use the **jira → code scaffold** form.

```
Idea:        ccli sandbox for chaos engineering
Project key: FLACO
```

**CLI:**

```bash
~/infra/flaco-v2 scaffold "ccli sandbox for chaos engineering" --project-key FLACO
```

⚠ Real Jira tickets get created. If you want to dry-run, unset
`JIRA_API_TOKEN` in `~/infra/flaco-v2.env` — flaco will still do the
local scaffold and mark Jira as skipped.

---

## Sidebars

* **Conversations** — every session flaco has had, auto-titled after the
  first user message.
* **Tool log** — live view of the last 20 tool calls, newest first. Show
  up within seconds.
* **Memory** — full list of everything flaco remembers about you. Type
  and press `+` to add a new fact.

---

## Terminal one-liners

```bash
# One-shot chat
flaco-v2 ask "what should I focus on this morning?"

# Interactive shiny TUI
flaco-v2 tui

# Run only the web UI
flaco-v2 web --web-port 3033

# Slack Socket Mode only (DON'T run alongside v1 — same bot token)
flaco-v2 slack
```

## Control

```bash
# Logs
tail -f mac-server:~/infra/flaco-v2-web.log

# PID
cat mac-server:~/infra/flaco-v2-web.pid

# Restart
ssh mac-server '
  kill $(cat ~/infra/flaco-v2-web.pid) 2>/dev/null
  nohup ~/infra/start-flaco-v2-web.sh > ~/infra/flaco-v2-web.log 2>&1 &
  echo $! > ~/infra/flaco-v2-web.pid
'

# Import markdown memory files as flaco facts
ssh mac-server '
  source ~/infra/flaco-v2.env
  FLACO_MODEL=qwen3:32b-q8_0 ~/infra/flaco-v2 \
    --db ~/infra/flaco.db import-memory --dir ~/infra/flaco-memory-seed
'
```

## v1 is still running

`~/infra/flacoai-server` on port **3031** is untouched. v2 runs on
**3033**. Slack app token is the same; only v1 is connected to Socket
Mode right now to avoid duplicate replies.

## Next up (follow-ups for Chris)

- Flip v2 Slack on once you're ready (`flaco-v2 slack`) and retire v1
- Vector embeddings for memory (`nomic-embed-text` drop-in)
- Hot-reloadable personas (currently baked into `persona.rs`)
- SSE token streaming in the web UI
- Per-channel conversation scoping in Slack v2 (currently per-user)
- Walter persona routing via `users:read.email` once the scope is added
