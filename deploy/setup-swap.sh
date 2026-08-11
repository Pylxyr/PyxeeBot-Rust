#!/usr/bin/env bash
set -euo pipefail

# One-time setup for a fresh host — e2.micro ships with zero swap by default.
SWAPFILE=/swapfile
SIZE_GB=2

if swapon --show | grep -q "$SWAPFILE"; then
    echo "Swap already active at $SWAPFILE, nothing to do."
    exit 0
fi

sudo fallocate -l "${SIZE_GB}G" "$SWAPFILE"
sudo chmod 600 "$SWAPFILE"
sudo mkswap "$SWAPFILE"
sudo swapon "$SWAPFILE"

if ! grep -q "^$SWAPFILE " /etc/fstab; then
    echo "$SWAPFILE none swap sw 0 0" | sudo tee -a /etc/fstab > /dev/null
fi

# Prefer keeping the bot's working set in RAM; only swap under real pressure.
sudo sysctl -w vm.swappiness=10
if ! grep -q "^vm.swappiness" /etc/sysctl.conf 2>/dev/null; then
    echo "vm.swappiness=10" | sudo tee -a /etc/sysctl.conf > /dev/null
fi

echo "Swap enabled: ${SIZE_GB}G at $SWAPFILE."
