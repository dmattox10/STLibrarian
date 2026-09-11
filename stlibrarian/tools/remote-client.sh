#!/usr/bin/env bash
#
# Remote-client check for STLibrarian Pro.
#
# tools/e2e.sh proves the API is correct. It talks to 127.0.0.1, which is the
# one client a NAS deployment never has. This script proves the server is
# usable *across a network*: the client runs in its own container, on its own
# network namespace, and reaches the server only by IP. Everything loopback
# hides -- the bind address, CORS, bearer tokens on the wire, whether a large
# file actually streams -- is only visible from here.
#
#   tools/remote-client.sh
#
# Requires docker for the remote half. Without it the script says so and stops,
# rather than quietly re-running e2e from localhost and calling it remote.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-38401}"
WORK="$(mktemp -d)"
SRV=""

# The address the container will use. Not 127.0.0.1 -- that is the whole point.
# The docker bridge gateway is the host as seen from a container: a real
# network hop, without leaving the machine or depending on what the firewall
# does to the public interface.
HOST_IP="${HOST_IP:-$(ip -4 addr show docker0 2>/dev/null | awk '/inet /{print $2}' | cut -d/ -f1)}"
[ -n "$HOST_IP" ] || HOST_IP="$(hostname -I | awk '{print $1}')"

CLIENT_IMAGE="${CLIENT_IMAGE:-curlimages/curl:8.11.1}"

cleanup() {
  if [ -n "$SRV" ]; then
    { kill "$SRV" && wait "$SRV"; } 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

pass=0
fail=0

check() {
  if [ "$#" -ne 3 ]; then
    printf '  \033[31mFAIL\033[0m  %s: check needs 3 arguments, got %d\n' "${1:-<none>}" "$#"
    fail=$((fail + 1))
    return
  fi
  if [ "$2" = "$3" ]; then
    printf '  \033[32mPASS\033[0m  %s (%s)\n' "$1" "$2"
    pass=$((pass + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s: got %s, want %s\n' "$1" "$2" "$3"
    fail=$((fail + 1))
  fi
}

note() { printf '  \033[33mNOTE\033[0m  %s\n' "$1"; }
head1() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# --- the remote client -------------------------------------------------------
#
# One container per call. Slower than a persistent one, and worth it: nothing
# can leak between calls, so a token that works is a token that genuinely
# travelled over the wire each time.
remote() {
  docker run --rm --network bridge "$CLIENT_IMAGE" \
    -s --max-time 120 "$@" 2>/dev/null
}

json() { sed -n "s/.*\"$1\":\"\?\([^,\"}]*\)\"\?.*/\1/p" <<<"$2" | head -1; }

# --- fixtures ----------------------------------------------------------------

LIB="$WORK/library"
mkdir -p "$LIB/Creator/Dragon"

# A binary STL of a cube, written by hand so the fixture needs no dependencies.
# 12 triangles, 84 + 50*12 bytes.
python3 - "$LIB/Creator/Dragon/body.stl" 10 <<'PY'
import struct, sys
path, mm = sys.argv[1], float(sys.argv[2])
v = [(0,0,0),(mm,0,0),(mm,mm,0),(0,mm,0),(0,0,mm),(mm,0,mm),(mm,mm,mm),(0,mm,mm)]
faces = [(0,3,2),(0,2,1),(4,5,6),(4,6,7),(0,1,5),(0,5,4),
         (1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,0,4),(3,4,7)]
with open(path,'wb') as f:
    f.write(b'\0'*80)
    f.write(struct.pack('<I', len(faces)))
    for a,b,c in faces:
        f.write(struct.pack('<3f', 0,0,0))
        for i in (a,b,c):
            f.write(struct.pack('<3f', *v[i]))
        f.write(struct.pack('<H', 0))
PY

# A big one, so "does it stream or does it buffer" is a real question.
python3 - "$LIB/Creator/Dragon/big.stl" <<'PY'
import struct, sys
path = sys.argv[1]
n = 400000          # 400k triangles, ~20 MB
with open(path,'wb') as f:
    f.write(b'\0'*80)
    f.write(struct.pack('<I', n))
    tri = struct.pack('<3f',0,0,1) + struct.pack('<9f', *([0,0,0, 1,0,0, 0,1,0])) + struct.pack('<H',0)
    for i in range(n):
        f.write(tri)
PY

BIG_SHA="$(sha256sum "$LIB/Creator/Dragon/big.stl" | cut -d' ' -f1)"
BIG_SIZE="$(stat -c %s "$LIB/Creator/Dragon/big.stl")"

# --- server ------------------------------------------------------------------

head1 "Building"
( cd "$HERE" && yarn build >/dev/null 2>&1 ) || { echo "  web build failed"; exit 1; }
( cd "$HERE/src-tauri" && cargo build --release --no-default-features \
    --features server --bin stlibrarian-server >/dev/null 2>&1 ) \
  || { echo "  server build failed"; exit 1; }
echo "  ok"

head1 "Starting server on 0.0.0.0:$PORT (client will use $HOST_IP)"
STLIBRARIAN_DATA_DIR="$WORK/data" \
STLIBRARIAN_WEB_ROOT="$HERE/dist" \
STLIBRARIAN_BIND="0.0.0.0:$PORT" \
  "$HERE/src-tauri/target/release/stlibrarian-server" > "$WORK/server.log" 2>&1 &
SRV=$!

for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1 && break
  sleep 0.2
done

ADMIN_PW="$(sed -n 's/^  password: //p' "$WORK/server.log" | head -1)"
[ -n "$ADMIN_PW" ] || { echo "  no bootstrap password in the log"; cat "$WORK/server.log"; exit 1; }
echo "  up, bootstrap admin password captured"

command -v docker >/dev/null || { echo "docker is required for the remote half"; exit 1; }
docker pull -q "$CLIENT_IMAGE" >/dev/null 2>&1 || true

BASE="http://$HOST_IP:$PORT"

# --- 1. reachable at all -----------------------------------------------------

head1 "Reachable from another host"
check "health over the network" \
  "$(remote "$BASE/api/health")" "ok"
check "bound beyond loopback" \
  "$(remote -o /dev/null -w '%{http_code}' "$BASE/api/health")" "200"

# --- 2. auth over the wire ---------------------------------------------------

head1 "Authentication, from a client that shares nothing with the server"
BODY="{\"username\":\"admin\",\"password\":\"$ADMIN_PW\"}"
LOGIN="$(remote -X POST "$BASE/api/login" -H 'Content-Type: application/json' -d "$BODY")"
TOKEN="$(json token "$LOGIN")"
check "login returns a token" "$([ -n "$TOKEN" ] && echo yes || echo no)" "yes"
check "and the user" "$(json username "$LOGIN")" "admin"

check "no token is 401" \
  "$(remote -o /dev/null -w '%{http_code}' "$BASE/api/models")" "401"
check "bad token is 401" \
  "$(remote -o /dev/null -w '%{http_code}' -H 'Authorization: Bearer nope' "$BASE/api/models")" "401"
check "the token works remotely" \
  "$(remote -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TOKEN" "$BASE/api/models")" "200"

AUTH=(-H "Authorization: Bearer $TOKEN")

# --- 3. CORS -----------------------------------------------------------------
#
# The desktop client pointed at a NAS is a webview on a different origin, and a
# browser will not send the request at all if the preflight is wrong. This
# cannot be tested from curl on localhost, because curl does not preflight.

head1 "CORS, as a browser on another machine would see it"
PRE="$(docker run --rm "$CLIENT_IMAGE" -s -i -X OPTIONS \
  -H "Origin: http://desktop.local:1420" \
  -H "Access-Control-Request-Method: GET" \
  -H "Access-Control-Request-Headers: authorization" \
  "$BASE/api/models" 2>/dev/null | tr -d '\r')"
check "preflight allows the origin" \
  "$(grep -ci 'access-control-allow-origin' <<<"$PRE")" "1"
check "  allows the Authorization header" \
  "$(grep -ci 'access-control-allow-headers' <<<"$PRE")" "1"
check "  allows the method" \
  "$(grep -ci 'access-control-allow-methods' <<<"$PRE")" "1"

# --- 4. the library ----------------------------------------------------------

head1 "Scanning and reading a library"
SCANBODY="{\"path\":\"$LIB\"}"
SCAN="$(remote "${AUTH[@]}" -X POST "$BASE/api/scan" -H 'Content-Type: application/json' -d "$SCANBODY")"
check "scan added both files" "$(json added "$SCAN")" "2"

MODELS="$(remote "${AUTH[@]}" "$BASE/api/models?sort=name")"
check "both listed remotely" "$(grep -o '"id":' <<<"$MODELS" | wc -l | tr -d ' ')" "2"

BIG_ID="$(python3 -c "
import json,sys
for m in json.load(sys.stdin):
    if m['name'].startswith('big'): print(m['id']); break
" <<<"$MODELS")"
SMALL_ID="$(python3 -c "
import json,sys
for m in json.load(sys.stdin):
    if m['name'].startswith('body'): print(m['id']); break
" <<<"$MODELS")"

# --- 5. file transfer --------------------------------------------------------
#
# The reason the desktop client can point at a NAS at all: it fetches the file
# and hands it to a local slicer. If that arrives corrupt or truncated, nothing
# else about the product matters.

head1 "Pulling a model across the network"
# Written on the host from the container's stdout. Mounting $WORK would be
# tidier but the image runs as a non-root user, so it cannot write into a
# mktemp directory -- and a download that fails on permissions would look
# exactly like a download that failed on the network.
remote "${AUTH[@]}" "$BASE/api/models/$BIG_ID/file" > "$WORK/big.stl"
check "downloaded byte-for-byte" \
  "$(sha256sum "$WORK/big.stl" | cut -d' ' -f1)" "$BIG_SHA"
check "  full length" "$(stat -c %s "$WORK/big.stl")" "$BIG_SIZE"

HDRS="$(remote -I "${AUTH[@]}" "$BASE/api/models/$BIG_ID/file" | tr -d '\r')"
check "  declares its length" \
  "$(grep -i '^content-length:' <<<"$HDRS" | awk '{print $2}')" "$BIG_SIZE"

# Resumable download: a 20 MB test file is polite, a real print farm's models
# are not. A client on flaky wifi needs Range or it starts over every time.
RANGE="$(remote -o /dev/null -w '%{http_code}' "${AUTH[@]}" \
  -H 'Range: bytes=0-1023' "$BASE/api/models/$BIG_ID/file")"
if [ "$RANGE" = "206" ]; then
  check "resumable (Range)" "$RANGE" "206"
else
  note "no Range support: a partial download restarts from zero (got $RANGE)"
fi

# --- 6. previews and thumbnails ---------------------------------------------

head1 "Previews, rendered on the server"
check "preview mesh serves" \
  "$(remote -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/models/$SMALL_ID/preview")" "200"

HASH="$(python3 -c "
import json,sys
for m in json.load(sys.stdin):
    if m['name'].startswith('body'): print(m['hash']); break
" <<<"$MODELS")"
remote "${AUTH[@]}" "$BASE/api/thumb/$HASH" > "$WORK/thumb.png" || true
check "thumbnail is a PNG" \
  "$(head -c 4 "$WORK/thumb.png" 2>/dev/null | od -An -tx1 | tr -d ' \n')" "89504e47"

# --- 7. two clients at once --------------------------------------------------

head1 "A second person, on a third machine"
UBODY='{"username":"sam","password":"correcthorsebattery","role":"user"}'
remote "${AUTH[@]}" -X POST "$BASE/api/users" -H 'Content-Type: application/json' -d "$UBODY" >/dev/null
SBODY='{"username":"sam","password":"correcthorsebattery"}'
SLOGIN="$(remote -X POST "$BASE/api/login" -H 'Content-Type: application/json' -d "$SBODY")"
STOKEN="$(json token "$SLOGIN")"
check "the second client signs in" "$([ -n "$STOKEN" ] && echo yes || echo no)" "yes"
check "  and sees the shared library" \
  "$(remote -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $STOKEN" "$BASE/api/models")" "200"
check "  but cannot scan" \
  "$(remote -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $STOKEN" \
     -H 'Content-Type: application/json' -d "$SCANBODY" "$BASE/api/scan")" "403"

# Both tokens live at once: one client signing in must not evict the other.
check "the first client is still signed in" \
  "$(remote -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/me")" "200"

# --- 8. the browser front end ------------------------------------------------

head1 "The web UI, fetched from another machine"
check "index serves" \
  "$(remote -o /dev/null -w '%{http_code}' "$BASE/")" "200"
check "a deep link still serves the app" \
  "$(remote -o /dev/null -w '%{http_code}' "$BASE/collections/3")" "200"
check "the JS bundle serves" \
  "$(remote -s -o /dev/null -w '%{http_code}' "$BASE$(remote "$BASE/" | sed -n 's/.*src="\([^"]*\.js\)".*/\1/p' | head -1)")" "200"
check "an unknown API path is still 404" \
  "$(remote -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/nope")" "404"

# A file that changed since the scan. The desktop client fetches this and
# hands it to a slicer, so a body that does not match its own Content-Length
# is not a header nit: the client writes whatever arrived and opens it.
head1 "A model edited since the last scan"
cat "$LIB/Creator/Dragon/body.stl" "$LIB/Creator/Dragon/body.stl" > "$WORK/grown"
mv "$WORK/grown" "$LIB/Creator/Dragon/body.stl"
GROWN_SIZE="$(stat -c %s "$LIB/Creator/Dragon/body.stl")"
remote "${AUTH[@]}" "$BASE/api/models/$SMALL_ID/file" > "$WORK/grown.stl"
check "the whole file arrives" "$(stat -c %s "$WORK/grown.stl")" "$GROWN_SIZE"
check "  and matches what is on disk" \
  "$(sha256sum "$WORK/grown.stl" | cut -d' ' -f1)" \
  "$(sha256sum "$LIB/Creator/Dragon/body.stl" | cut -d' ' -f1)"

# --- 9. logout ---------------------------------------------------------------

head1 "Signing out"
remote "${AUTH[@]}" -X POST "$BASE/api/logout" >/dev/null
check "the token dies server-side" \
  "$(remote -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/models")" "401"
check "  the other client is unaffected" \
  "$(remote -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $STOKEN" "$BASE/api/models")" "200"

# --- result ------------------------------------------------------------------

printf '\n\033[1m%d passed, %d failed\033[0m\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
