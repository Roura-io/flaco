#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# flacoAi installer — double-click this file on macOS to begin
# ─────────────────────────────────────────────────────────────

# cd to the directory this file lives in (handles double-click from Finder)
cd "$(dirname "$0")" || exit 1

# Make sure setup.sh is executable
chmod +x ./setup.sh

# Run the interactive installer
./setup.sh

# Keep Terminal open so the user can read the results
echo ""
echo "Press any key to close this window..."
read -n 1 -s -r
