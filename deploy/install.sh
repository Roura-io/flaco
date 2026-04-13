#!/usr/bin/env bash
# Deploy the flaco-v2 binary to mac-server, re-sign it (macOS otherwise
# SIGKILLs the new binary on launch when an existing executable is replaced
# in place), and restart the web service.
#
# Usage:
#   ./deploy/install.sh           # builds + ships + restarts
#   ./deploy/install.sh --build   # just build locally
#   ./deploy/install.sh --no-build # ship existing release/flaco-v2

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO/rust/target/release/flaco-v2"
SSH_TARGET="${MAC_SSH:-mac-server}"
REMOTE_BIN="/Users/roura.io.server/infra/flaco-v2"

build() {
  echo "› cargo build --release -p flaco-v2"
  ( cd "$REPO/rust" && cargo build --release -p flaco-v2 )
}

ship() {
  echo "› scp $BIN -> $SSH_TARGET:$REMOTE_BIN"
  scp -q "$BIN" "$SSH_TARGET:$REMOTE_BIN"
  echo "› re-sign in place (workaround for macOS replace-exec SIGKILL)"
  ssh "$SSH_TARGET" "codesign --remove-signature $REMOTE_BIN 2>/dev/null; codesign --force --sign - $REMOTE_BIN"
}

restart() {
  echo "› restart web service"
  ssh "$SSH_TARGET" '
    if [ -f ~/infra/flaco-v2-web.pid ]; then
      kill $(cat ~/infra/flaco-v2-web.pid) 2>/dev/null || true
    fi
    sleep 1
    nohup ~/infra/start-flaco-v2-web.sh > ~/infra/flaco-v2-web.log 2>&1 &
    echo $! > ~/infra/flaco-v2-web.pid
    sleep 2
    curl -s --max-time 3 http://localhost:3033/health
    echo
  '
}

case "${1:-}" in
  --build)    build ;;
  --no-build) ship; restart ;;
  *)          build; ship; restart ;;
esac
