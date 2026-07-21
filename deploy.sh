#!/usr/bin/env bash
set -euo pipefail

# Manual fallback for release.yml's automated deploy (which runs on every
# push to main). Kept in sync with it: same service name, same path.

: "${SERVER_HOST:?Set SERVER_HOST to the Oracle instance address}"
: "${SERVER_USER:?Set SERVER_USER (e.g. ubuntu)}"

cd "$(dirname "$0")"

echo "Building release binary..."
cargo build --release

echo "Uploading binary..."
scp target/release/pyxeebot "${SERVER_USER}@${SERVER_HOST}:~/pyxeebotr/pyxeebot.new"

echo "Swapping binary and restarting service..."
ssh "${SERVER_USER}@${SERVER_HOST}" '
  set -e
  mv ~/pyxeebotr/pyxeebot.new ~/pyxeebotr/pyxeebot
  chmod +x ~/pyxeebotr/pyxeebot
  sudo systemctl restart pyxeebotr
  sleep 2
  sudo systemctl is-active --quiet pyxeebotr && echo "pyxeebotr is running" || {
    echo "pyxeebotr failed to start — check: journalctl -u pyxeebotr -n 50"
    exit 1
  }
'

echo "Deploy complete."
