#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# flacoAi installer — double-click this file on macOS to begin
# ─────────────────────────────────────────────────────────────

# cd to the directory this file lives in (handles double-click from Finder)
cd "$(dirname "$0")" || exit 1

# Find the setup script (hidden in release builds, visible in dev)
if [[ -f ./.setup.sh ]]; then
    SETUP="./.setup.sh"
else
    SETUP="./setup.sh"
fi

chmod +x "$SETUP"

# Run the interactive installer
"$SETUP"

# Keep Terminal open so the user can read the results
echo ""
echo "Press any key to close this window..."
read -n 1 -s -r
