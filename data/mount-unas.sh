#!/bin/bash
# Mount the UNAS roura.io.private share at ~/mnt/Roura.io
# Called by launchd on boot or manually before starting flacoAi.
# Uses SMB with the rouraio user account.

MOUNT_POINT="$HOME/mnt/Roura.io"
SMB_URL="//rouraio:Scorpion011625%23@10.0.1.2/roura.io.private"

if mount | grep -q "$MOUNT_POINT"; then
    echo "UNAS already mounted at $MOUNT_POINT"
    exit 0
fi

mkdir -p "$MOUNT_POINT"
mount_smbfs "$SMB_URL" "$MOUNT_POINT"
STATUS=$?

if [ $STATUS -eq 0 ]; then
    echo "UNAS mounted at $MOUNT_POINT"
else
    echo "Failed to mount UNAS (exit $STATUS)" >&2
    exit $STATUS
fi
