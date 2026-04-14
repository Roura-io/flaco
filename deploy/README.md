# flaco-v2 deployment

## Files

- `io.roura.flaco-v2.plist` — launchd spec that runs the web UI at login
  and restarts on crash.

## Install (optional — currently running under nohup)

```bash
# Copy the plist to user LaunchAgents
scp deploy/io.roura.flaco-v2.plist \
  mac-server:~/Library/LaunchAgents/io.roura.flaco-v2.plist

# Stop the nohup instance so launchd can take over
ssh mac-server '
  if [ -f ~/infra/flaco-v2-web.pid ]; then
    kill $(cat ~/infra/flaco-v2-web.pid) 2>/dev/null || true
    rm ~/infra/flaco-v2-web.pid
  fi
  launchctl load -w ~/Library/LaunchAgents/io.roura.flaco-v2.plist
  sleep 2
  curl -s http://localhost:3033/health && echo
'
```

## Uninstall

```bash
ssh mac-server '
  launchctl unload -w ~/Library/LaunchAgents/io.roura.flaco-v2.plist
  rm ~/Library/LaunchAgents/io.roura.flaco-v2.plist
'
```

## Current runtime (not launchd, nohup for hackathon)

```
binary:     ~/infra/flaco-v2           (release build, 11 MB)
env file:   ~/infra/flaco-v2.env       (mode 600)
start:      ~/infra/start-flaco-v2-web.sh
pid:        ~/infra/flaco-v2-web.pid
log:        ~/infra/flaco-v2-web.log
db:         ~/infra/flaco.db           (sqlite with FTS5)
url:        http://mac.home:3033
seed dir:   ~/infra/claude-memory-seed (copy of Claude Code auto-memory)
```
