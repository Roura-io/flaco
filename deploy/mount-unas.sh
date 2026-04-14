#!/usr/bin/env bash
# mount-unas.sh — attach the UniFi Drive "Roura.io" shared drive to
# mac-server so flaco's save_to_unas tool has a writable target.
#
# Run once interactively to set up:
#
#   1. Create a UniFi Identity user dedicated to flaco (e.g. "flaco")
#      with write access to the Roura.io shared drive.
#   2. In the UniFi admin UI → that user → File Services & Time
#      Machine Credentials → generate an SMB password. Copy it.
#   3. On mac-server (via SSH):
#
#      export FLACO_UNAS_SMB_USER="flaco"
#      export FLACO_UNAS_SMB_PASS="<the smb password>"
#      export FLACO_UNAS_SMB_HOST="10.0.1.2"
#      export FLACO_UNAS_SMB_SHARE="Roura.io"
#      bash ~/infra/mount-unas.sh
#
#   4. The first run adds the credentials to the macOS login keychain
#      (so future mounts don't need the password again) and mounts
#      the share at /Volumes/Roura.io. After that, re-runs are idempotent —
#      if the mount is already present, it's a no-op.
#
# The script is intentionally simple and synchronous. Auto-mount on
# boot is a separate concern: wrap this in a LaunchAgent plist once
# you know the credentials work end-to-end.

set -euo pipefail

UNAS_HOST="${FLACO_UNAS_SMB_HOST:-10.0.1.2}"
UNAS_SHARE="${FLACO_UNAS_SMB_SHARE:-Roura.io}"
UNAS_USER="${FLACO_UNAS_SMB_USER:-}"
UNAS_PASS="${FLACO_UNAS_SMB_PASS:-}"
MOUNT_POINT="${FLACO_UNAS_MOUNT:-/Volumes/Roura.io}"

say() { printf '[mount-unas] %s\n' "$*"; }
fail() { printf '[mount-unas] ERROR: %s\n' "$*" >&2; exit 1; }

# Already mounted? Idempotent success.
if mount | grep -q "on ${MOUNT_POINT} "; then
  say "already mounted at ${MOUNT_POINT}"
  exit 0
fi

if [[ -z "${UNAS_USER}" || -z "${UNAS_PASS}" ]]; then
  fail "FLACO_UNAS_SMB_USER and FLACO_UNAS_SMB_PASS must be set. See comment at top of this script."
fi

# Make sure the mount point exists and is an empty directory.
# Do NOT delete an existing non-empty directory — that's how local
# data gets silently shadowed by an SMB mount and you lose it later.
if [[ -e "${MOUNT_POINT}" && ! -d "${MOUNT_POINT}" ]]; then
  fail "${MOUNT_POINT} exists but is not a directory. Aborting so we don't clobber something."
fi
if [[ -d "${MOUNT_POINT}" ]]; then
  if [[ -n "$(ls -A "${MOUNT_POINT}" 2>/dev/null)" ]]; then
    fail "${MOUNT_POINT} exists and is not empty. Aborting so an SMB mount can't shadow local files."
  fi
else
  mkdir -p "${MOUNT_POINT}"
fi

# Persist the credential in the macOS keychain so subsequent mounts
# don't re-prompt. This is a one-time add per (host, user) pair.
# If it already exists, `security add-internet-password` errors; we
# swallow that because it means we're already set up.
say "storing SMB credential in macOS login keychain (if not already present)"
security add-internet-password \
  -a "${UNAS_USER}" \
  -s "${UNAS_HOST}" \
  -r "smb " \
  -p "${UNAS_SHARE}" \
  -D "Network Password" \
  -w "${UNAS_PASS}" \
  -T /sbin/mount_smbfs \
  2>/dev/null || say "credential already present (ok)"

say "mounting //${UNAS_USER}@${UNAS_HOST}/${UNAS_SHARE} → ${MOUNT_POINT}"

# URL-encode the password for the mount URL.
ENCODED_PASS=$(python3 -c "import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "${UNAS_PASS}")

mount_smbfs "//${UNAS_USER}:${ENCODED_PASS}@${UNAS_HOST}/${UNAS_SHARE}" "${MOUNT_POINT}"

# Verify
if mount | grep -q "on ${MOUNT_POINT} "; then
  say "mounted OK"
  ls -la "${MOUNT_POINT}" | head -10
  exit 0
else
  fail "mount_smbfs returned 0 but nothing is mounted at ${MOUNT_POINT}. Check console.log for details."
fi
