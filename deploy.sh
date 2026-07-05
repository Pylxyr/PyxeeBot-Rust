#!/usr/bin/env bash
set -euo pipefail

: "${SERVER_HOST:?Set SERVER_HOST to the Oracle instance address}"
: "${SERVER_USER:?Set SERVER_USER (e.g. ubuntu)}"

cd "$(dirname "$0")"

echo "Building release binary..."
cargo build --release

echo "Uploading binary..."
scp target/release/pyxeebot "${SERVER_USER}@${SERVER_HOST}:~/musicbot/pyxeebot.new"

echo "Swapping binary and restarting service..."
ssh "${SERVER_USER}@${SERVER_HOST}" '
  set -e
  mv ~/musicbot/pyxeebot.new ~/musicbot/pyxeebot
  chmod +x ~/musicbot/pyxeebot
  sudo systemctl restart musicbot
  sleep 2
  sudo systemctl is-active --quiet musicbot && echo "musicbot is running" || {
    echo "musicbot failed to start — check: journalctl -u musicbot -n 50"
    exit 1
  }
'

echo "Deploy complete."
