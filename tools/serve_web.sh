#!/usr/bin/env bash
# Serve the browser client: the relay (WebSocket <-> UDP) and a static web server.
#
#   tools/serve_web.sh            build if needed, then serve on http://localhost:8080
#   tools/serve_web.sh --build    rebuild the wasm client first
#
# Open:
#   http://localhost:8080/?server=127.0.0.1:2190&relay=ws://127.0.0.1:2290&account=<id>&sid=<session>
# The game server (experimental, run_local_2018.sh) must already be running.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
WEB_PORT="${WEB_PORT:-8080}"
RELAY="${RELAY:-0.0.0.0:2290}"

if [ "${1:-}" = "--build" ] || [ ! -f client/dist/index.html ]; then
    (cd client && trunk build --release)
fi
# The converted game data is never copied: the page reads it through a symlink.
ln -sfn ../../assets client/dist/assets

cargo build -q --release -p dsor-relay
./target/release/dsor-relay --listen "$RELAY" &
RELAY_PID=$!
trap 'kill $RELAY_PID 2>/dev/null' EXIT
echo "relay on ws://${RELAY}, web on http://localhost:${WEB_PORT}"
python3 -m http.server "$WEB_PORT" --directory client/dist
