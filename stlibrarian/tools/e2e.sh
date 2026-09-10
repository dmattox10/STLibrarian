#!/usr/bin/env bash
#
# End-to-end check for STLibrarian Pro.
#
# Builds the server and the web front end, starts the server against a scratch
# data directory on a scratch port, and exercises the API the way a client
# does. Leaves nothing behind and never touches your real library.
#
#   tools/e2e.sh
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-38399}"
WORK="$(mktemp -d)"
SRV=""

cleanup() {
  if [ -n "$SRV" ]; then
    # Job-control chatter goes to the shell's stderr, not the kill's, so both
    # the signal and the reap are silenced here.
    { kill "$SRV" && wait "$SRV"; } 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

pass=0
fail=0
check() {
  if [ "$2" = "$3" ]; then
    printf '  \033[32mPASS\033[0m  %s (%s)\n' "$1" "$2"
    pass=$((pass + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s: got %s, want %s\n' "$1" "$2" "$3"
    fail=$((fail + 1))
  fi
}
code() { curl -s -o /dev/null -w "%{http_code}" "$@"; }

echo "==> building"
(cd "$HERE" && yarn build >/dev/null)
(cd "$HERE/src-tauri" && cargo build --quiet --no-default-features --features server --bin stlibrarian-server)

echo "==> fixtures"
mkdir -p "$WORK/models/nested"
python3 - "$WORK/models" <<'PY'
import struct, sys, pathlib
root = pathlib.Path(sys.argv[1])
def stl(tris):
    out = bytearray(b"\0" * 80) + struct.pack("<I", len(tris))
    for t in tris:
        out += b"\0" * 12
        for v in t:
            out += struct.pack("<3f", *v)
        out += b"\0\0"
    return bytes(out)
(root / "widget.stl").write_bytes(stl([[(0, 0, 0), (10, 0, 0), (0, 20, 5)]]))
# Uppercase extension: the scanner must find this too.
(root / "nested" / "BRACKET.STL").write_bytes(
    stl([[(0, 0, 0), (3, 0, 0), (0, 4, 12)], [(1, 1, 1), (2, 2, 2), (3, 3, 3)]])
)
PY

echo "==> starting server on :$PORT"
export STLIBRARIAN_DATA_DIR="$WORK/data"
export STLIBRARIAN_BIND="127.0.0.1:$PORT"
export STLIBRARIAN_WEB_ROOT="$HERE/dist"
"$HERE/src-tauri/target/debug/stlibrarian-server" > "$WORK/server.log" 2>&1 &
SRV=$!
curl -s --retry 30 --retry-delay 1 --retry-connrefused "http://127.0.0.1:$PORT/api/health" >/dev/null

BASE="http://127.0.0.1:$PORT"
PASS_WORD="$(grep 'password:' "$WORK/server.log" | awk '{print $2}')"
TOKEN="$(curl -s -X POST "$BASE/api/login" -H 'Content-Type: application/json' \
  -d "{\"username\":\"admin\",\"password\":\"$PASS_WORD\"}" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')"
AUTH=(-H "Authorization: Bearer $TOKEN")
jlen() { python3 -c 'import sys,json;print(len(json.load(sys.stdin)))'; }

echo "== routing =="
check "GET /"                   "$(code "$BASE/")" 200
check "SPA deep link keeps 200" "$(code "$BASE/library/42")" 200
check "unknown API is 404"      "$(code "$BASE/api/nope")" 404
check "  and is JSON"           "$(curl -s "$BASE/api/nope" | head -c 1)" "{"

echo "== auth =="
check "no token rejected"       "$(code "$BASE/api/models")" 401
check "wrong password"          "$(code -X POST "$BASE/api/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"x"}')" 401
check "admin/admin is dead"     "$(code -X POST "$BASE/api/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"admin"}')" 401
check "generated password is not 'admin'" "$([ "$PASS_WORD" = admin ] && echo yes || echo no)" "no"
check "authenticated read"      "$(code "${AUTH[@]}" "$BASE/api/models")" 200

echo "== scanning =="
curl -s "${AUTH[@]}" -X POST "$BASE/api/scan" -H 'Content-Type: application/json' \
  -d "{\"path\":\"$WORK/models\"}" >/dev/null
check "found both STLs (incl. .STL)" "$(curl -s "${AUTH[@]}" "$BASE/api/models" | jlen)" 2
ID="$(curl -s "${AUTH[@]}" "$BASE/api/models?sort=name" | python3 -c 'import sys,json;print(json.load(sys.stdin)[0]["id"])')"
HASH="$(curl -s "${AUTH[@]}" "$BASE/api/models?sort=name" | python3 -c 'import sys,json;print(json.load(sys.stdin)[0]["hash"])')"
check "bounding box measured"   "$(curl -s "${AUTH[@]}" "$BASE/api/models/$ID" | python3 -c 'import sys,json;m=json.load(sys.stdin);print("%gx%gx%g" % (m["dim_x"], m["dim_y"], m["dim_z"]))')" "3x4x12"
check "file streams back"       "$(curl -s "${AUTH[@]}" "$BASE/api/models/$ID/file" | wc -c | tr -d ' ')" 184

echo "== tags =="
curl -s "${AUTH[@]}" -X POST "$BASE/api/models/$ID/tags" -H 'Content-Type: application/json' \
  -d '{"tags":["dragons","minis"]}' >/dev/null
check "filter matches"          "$(curl -s "${AUTH[@]}" "$BASE/api/models?tags=dragons" | jlen)" 1
check "filter is AND not OR"    "$(curl -s "${AUTH[@]}" "$BASE/api/models?tags=dragons,nope" | jlen)" 0
check "search"                  "$(curl -s "${AUTH[@]}" "$BASE/api/models?search=BRACK" | jlen)" 1

echo "== thumbnails =="
PNG='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
check "store preview"           "$(code "${AUTH[@]}" -X PUT "$BASE/api/thumb/$HASH" -H 'Content-Type: application/json' -d "{\"data_url\":\"$PNG\"}")" 204
check "read preview"            "$(code "${AUTH[@]}" "$BASE/api/thumb/$HASH")" 200
check "path traversal refused"  "$(code "${AUTH[@]}" "$BASE/api/thumb/..%2f..%2f..%2fetc%2fpasswd")" 404
check "non-PNG refused"         "$(code "${AUTH[@]}" -X PUT "$BASE/api/thumb/$HASH" -H 'Content-Type: application/json' -d '{"data_url":"data:image/png;base64,R0lGODlhAQABAA=="}')" 500

echo "== rescan =="
check "idempotent"              "$(curl -s "${AUTH[@]}" -X POST "$BASE/api/scan" -H 'Content-Type: application/json' -d "{\"path\":\"$WORK/models\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["unchanged"])')" 2
check "user tags survive"       "$(curl -s "${AUTH[@]}" "$BASE/api/models/$ID" | python3 -c 'import sys,json;print(",".join(sorted(json.load(sys.stdin)["tags"])))')" "dragons,minis"

echo
if [ "$fail" -gt 0 ]; then
  echo "RESULT FAIL - $pass passed, $fail failed"
  exit 1
fi
echo "RESULT PASS - $pass checks"
