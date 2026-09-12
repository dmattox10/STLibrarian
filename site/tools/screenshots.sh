#!/usr/bin/env bash
# Regenerate the three screenshots on stlibrarian.com, end to end.
#
#   site/tools/screenshots.sh
#
# The shots must be of the real application or they are worth nothing, so this
# builds the headless server, gives it a throwaway library of generated meshes,
# and photographs the real UI in a real browser. Nothing is mocked and no DOM
# is doctored.
#
# Re-run it whenever the interface changes. It is destructive only inside its
# own scratch directory and $LIBRARY, never the repo and never a real library.
#
# Needs: cargo, node, yarn, ffmpeg, and Playwright's chromium. On a machine
# without Playwright installed, `npx playwright install chromium` first and
# point PLAYWRIGHT at the playwright-core it drops in the npx cache.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$(cd "$HERE/../../stlibrarian" && pwd)"
SCRATCH="${SCRATCH:-/tmp/stlibrarian-shots}"
# Deliberately the path the app's own placeholder suggests. The library path is
# on screen in the details panel, and a screenshot showing somebody's build
# directory is a screenshot of a test rig rather than of a product.
LIBRARY="${LIBRARY:-/volume1/3dprints}"
PORT="${PORT:-3999}"
BIN="$APP/src-tauri/target/release/stlibrarian-server"
# seed.mjs and capture.mjs read this, so PORT stays settable in one place.
export STLIBRARIAN_URL="http://127.0.0.1:$PORT"

say() { printf '\n==> %s\n' "$*"; }

say "Building the front end"
cd "$APP"
yarn install --frozen-lockfile
yarn build

say "Building the headless server"
cd "$APP/src-tauri"
cargo build --release --no-default-features --features server --bin stlibrarian-server

say "Generating the sample library into $LIBRARY"
rm -rf "$LIBRARY"
node "$HERE/make-models.mjs" "$LIBRARY"

say "Starting the server on 127.0.0.1:$PORT with a throwaway database"
# Bound to localhost on purpose: the default is 0.0.0.0, and this server holds
# a bootstrap admin account with a password that is about to be echoed to a log.
rm -rf "$SCRATCH"
mkdir -p "$SCRATCH"
STLIBRARIAN_DATA_DIR="$SCRATCH/data" \
STLIBRARIAN_BIND="127.0.0.1:$PORT" \
STLIBRARIAN_WEB_ROOT="$APP/dist" \
  "$BIN" > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true' EXIT

for _ in $(seq 1 40); do
  sleep 0.5
  curl -fsS "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1 && break
done
curl -fsS "http://127.0.0.1:$PORT/api/health" >/dev/null

# Printed once on first run and never stored in plaintext, so it is read back
# out of the log rather than chosen here.
PASSWORD="$(sed -n 's/^  password: //p' "$SCRATCH/server.log" | head -1)"
[ -n "$PASSWORD" ] || { echo "no bootstrap password in $SCRATCH/server.log" >&2; exit 1; }

say "Scanning and seeding"
TOKEN="$(curl -fsS -X POST "http://127.0.0.1:$PORT/api/login" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"admin\",\"password\":\"$PASSWORD\"}" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')"

curl -fsS -X POST "http://127.0.0.1:$PORT/api/scan" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"path\":\"$LIBRARY\"}"
echo
node "$HERE/seed.mjs" "$TOKEN"

say "Photographing the UI"
node "$HERE/capture.mjs" "$PASSWORD"

say "Downsampling for the web"
# 1600px wide is a little over the ~1160px the page gives them at full width,
# so they stay crisp without shipping a 2x file for a dense screenshot.
for name in library model prints; do
  ffmpeg -y -loglevel error \
    -i "$HERE/out/$name.png" \
    -vf "scale=1600:-2:flags=lanczos" -q:v 4 \
    "$HERE/../shots/$name.jpg"
  printf '    %-12s %s\n' "$name.jpg" "$(du -h "$HERE/../shots/$name.jpg" | cut -f1)"
done

say "Done. site/shots/ updated; review them before committing."
