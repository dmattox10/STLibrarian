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
# A note on style, learned the hard way: JSON bodies are built into shell
# variables first and never written inline as "{\"a\":\"b\"}" inside a $(...)
# that is itself inside double quotes. In that position bash strips the
# backslashes, the braces end up unquoted, and a comma triggers brace
# expansion -- which silently splits the request in two AND eats the argument
# after it, so a check can run with the expected value missing.
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
  # Arity guard. A quoting mistake upstream can drop the expected value, and
  # comparing "" against a missing "" would report a cheerful PASS.
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

# --- request helpers. Payloads arrive as a single already-built argument. ---
api()      { curl -s "${AUTH[@]}" "$@"; }
api_code() { curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$@"; }
post()     { curl -s "${AUTH[@]}" -X POST "$1" -H 'Content-Type: application/json' -d "$2"; }
post_code(){ curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X POST "$1" -H 'Content-Type: application/json' -d "$2"; }
put_code() { curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X PUT "$1" -H 'Content-Type: application/json' -d "$2"; }
patch_code(){ curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X PATCH "$1" -H 'Content-Type: application/json' -d "$2"; }

# JSON readers take their key via argv, so no quotes need nesting.
jfield() { python3 -c 'import sys,json;print(json.load(sys.stdin)[sys.argv[1]])' "$1"; }
jlen()   { python3 -c 'import sys,json;print(len(json.load(sys.stdin)))'; }
jfirst() { python3 -c 'import sys,json;print(json.load(sys.stdin)[0][sys.argv[1]])' "$1"; }
jjoin()  { python3 -c 'import sys,json;print(",".join(sorted(json.load(sys.stdin)[sys.argv[1]])))' "$1"; }

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
ADMIN_PW="$(grep 'password:' "$WORK/server.log" | awk '{print $2}')"
# Seeded non-empty: macOS ships bash 3.2, where "${arr[@]}" on an empty array
# is an "unbound variable" error under `set -u`.
AUTH=(-H "X-STLibrarian-Test: e2e")
LOGIN_BODY="$(printf '{"username":"admin","password":"%s"}' "$ADMIN_PW")"
TOKEN="$(post "$BASE/api/login" "$LOGIN_BODY" | jfield token)"
AUTH=(-H "Authorization: Bearer $TOKEN")

echo "== routing =="
check "GET /"                   "$(api_code "$BASE/")" 200
check "SPA deep link keeps 200" "$(api_code "$BASE/library/42")" 200
check "unknown API is 404"      "$(api_code "$BASE/api/nope")" 404
check "  and is JSON"           "$(api "$BASE/api/nope" | head -c 1)" "{"

echo "== auth =="
NO_AUTH_CODE="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/models")"
check "no token rejected"       "$NO_AUTH_CODE" 401
check "wrong password"          "$(post_code "$BASE/api/login" '{"username":"admin","password":"x"}')" 401
check "admin/admin is dead"     "$(post_code "$BASE/api/login" '{"username":"admin","password":"admin"}')" 401
check "generated pw is not 'admin'" "$([ "$ADMIN_PW" = admin ] && echo yes || echo no)" "no"
check "token length"            "${#TOKEN}" 64
check "authenticated read"      "$(api_code "$BASE/api/models")" 200

echo "== scanning =="
SCAN_BODY="$(printf '{"path":"%s"}' "$WORK/models")"
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null
check "found both STLs (incl. .STL)" "$(api "$BASE/api/models" | jlen)" 2
ID="$(api "$BASE/api/models?sort=name" | jfirst id)"
HASH="$(api "$BASE/api/models?sort=name" | jfirst hash)"
check "sorted by name"          "$(api "$BASE/api/models?sort=name" | jfirst name)" "BRACKET"
check "bounding box measured"   "$(api "$BASE/api/models/$ID" | python3 -c 'import sys,json;m=json.load(sys.stdin);print("%gx%gx%g" % (m["dim_x"],m["dim_y"],m["dim_z"]))')" "3x4x12"
check "file streams back"       "$(api "$BASE/api/models/$ID/file" | wc -c | tr -d ' ')" 184

echo "== tags =="
TAGS_BODY='{"tags":["dragons","minis"]}'
post "$BASE/api/models/$ID/tags" "$TAGS_BODY" >/dev/null
check "filter matches"          "$(api "$BASE/api/models?tags=dragons" | jlen)" 1
check "filter is AND not OR"    "$(api "$BASE/api/models?tags=dragons,nope" | jlen)" 0
check "search"                  "$(api "$BASE/api/models?search=BRACK" | jlen)" 1

echo "== thumbnails =="
PNG_BODY='{"data_url":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}'
check "store preview"           "$(put_code "$BASE/api/thumb/$HASH" "$PNG_BODY")" 204
check "read preview"            "$(api_code "$BASE/api/thumb/$HASH")" 200
check "path traversal refused"  "$(api_code "$BASE/api/thumb/..%2f..%2f..%2fetc%2fpasswd")" 404
check "non-PNG refused"         "$(put_code "$BASE/api/thumb/$HASH" '{"data_url":"data:image/png;base64,R0lGODlhAQABAA=="}')" 500

echo "== collections =="
CID="$(post "$BASE/api/collections" '{"name":"Dragons"}' | jfield id)"
check "created"                 "$([ -n "$CID" ] && echo yes || echo no)" "yes"
check "duplicate name refused"  "$(post_code "$BASE/api/collections" '{"name":"dragons"}')" 400
check "blank name refused"      "$(post_code "$BASE/api/collections" '{"name":"   "}')" 400

ALL_IDS="$(api "$BASE/api/models" | python3 -c 'import sys,json;print(json.dumps([m["id"] for m in json.load(sys.stdin)],separators=(",",":")))')"
MEMBERS_BODY="$(printf '{"model_ids":%s}' "$ALL_IDS")"
ONE_BODY="$(printf '{"model_ids":[%s]}' "$ID")"
check "added both"              "$(post "$BASE/api/collections/$CID/models" "$MEMBERS_BODY" | jfield changed)" 2
check "re-adding changes none"  "$(post "$BASE/api/collections/$CID/models" "$MEMBERS_BODY" | jfield changed)" 0
check "collection count"        "$(api "$BASE/api/collections" | jfirst count)" 2
check "filter by collection"    "$(api "$BASE/api/models?collection=$CID" | jlen)" 2
check "collection AND search"   "$(api "$BASE/api/models?collection=$CID&search=widget" | jlen)" 1
check "membership on the model" "$(api "$BASE/api/models/$ID" | jjoin collections)" "Dragons"
check "renamed"                 "$(patch_code "$BASE/api/collections/$CID" '{"name":"Big Lizards"}')" 204
check "  name changed"          "$(api "$BASE/api/collections" | jfirst name)" "Big Lizards"
check "  members survived"      "$(api "$BASE/api/collections" | jfirst count)" 2
check "removed one"             "$(post "$BASE/api/collections/$CID/models/remove" "$ONE_BODY" | jfield changed)" 1
check "  count dropped"         "$(api "$BASE/api/collections" | jfirst count)" 1

echo "== bulk tagging =="
# $ID already carries "minis" from the tags section, so only the other model
# changes -- which is exactly the "already had it" case worth proving.
BULK_MINIS="$(printf '{"model_ids":%s,"tag":"minis"}' "$ALL_IDS")"
BULK_ONE="$(printf '{"model_ids":[%s],"tag":"minis"}' "$ID")"
BULK_UNKNOWN="$(printf '{"model_ids":%s,"tag":"never-existed"}' "$ALL_IDS")"
check "tags the ones that lack it" "$(post "$BASE/api/bulk/tags/add" "$BULK_MINIS" | jfield changed)" 1
check "  now both carry it"     "$(api "$BASE/api/models?tags=minis" | jlen)" 2
check "  existing tags kept"    "$(api "$BASE/api/models/$ID" | jjoin tags)" "dragons,minis"
check "re-adding changes none"  "$(post "$BASE/api/bulk/tags/add" "$BULK_MINIS" | jfield changed)" 0
check "remove from one only"    "$(post "$BASE/api/bulk/tags/remove" "$BULK_ONE" | jfield changed)" 1
check "  the other still has it" "$(api "$BASE/api/models?tags=minis" | jlen)" 1
check "unknown tag removes none" "$(post "$BASE/api/bulk/tags/remove" "$BULK_UNKNOWN" | jfield changed)" 0

echo "== deleting a collection =="
check "deleted"                 "$(api_code -X DELETE "$BASE/api/collections/$CID")" 204
check "  models untouched"      "$(api "$BASE/api/models" | jlen)" 2
check "  membership cleared"    "$(api "$BASE/api/models/$ID" | jjoin collections)" ""

echo "== rescan =="
check "idempotent"              "$(post "$BASE/api/scan" "$SCAN_BODY" | jfield unchanged)" 2
check "user tags survive"       "$(api "$BASE/api/models/$ID" | jjoin tags)" "dragons"


# Deliberately last: this adds a third model, and every count assertion above
# is written against the two fixtures.
echo "== volume =="
# The fixtures are open shells, so build a closed cube to measure.
python3 - "$WORK/models/cube.stl" <<'PYCUBE'
import struct, sys, pathlib
s = 10.0
p = [[0,0,0],[s,0,0],[s,s,0],[0,s,0],[0,0,s],[s,0,s],[s,s,s],[0,s,s]]
faces = [(0,1,2),(0,2,3),(4,6,5),(4,7,6),(0,5,1),(0,4,5),
         (1,6,2),(1,5,6),(2,7,3),(2,6,7),(3,4,0),(3,7,4)]
out = bytearray(b"\0"*80) + struct.pack("<I", len(faces))
for a,b,c in faces:
    out += b"\0"*12
    for v in (p[a],p[b],p[c]): out += struct.pack("<3f", *v)
    out += b"\0\0"
pathlib.Path(sys.argv[1]).write_bytes(bytes(out))
PYCUBE
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null
CUBE_ID="$(api "$BASE/api/models?search=cube" | jfirst id)"
check "10mm cube is 1000 mm3" "$(api "$BASE/api/models/$CUBE_ID" | python3 -c 'import sys,json;print(round(json.load(sys.stdin)["volume"]))')" 1000

echo "== server-side rendering =="
# No PUT first: the server has to rasterise this itself, which is the whole
# point -- a headless NAS has no browser to make previews for it.
CUBE_HASH="$(api "$BASE/api/models?search=cube" | jfirst hash)"
check "rendered on demand"      "$(api_code "$BASE/api/thumb/$CUBE_HASH")" 200
check "  is a PNG"              "$(api "$BASE/api/thumb/$CUBE_HASH" | head -c 4 | tail -c 3)" "PNG"
check "  cached to disk"        "$(ls "$WORK/data/thumbnails" | grep -c "$CUBE_HASH")" 1


echo
if [ "$fail" -gt 0 ]; then
  echo "RESULT FAIL - $pass passed, $fail failed"
  exit 1
fi
echo "RESULT PASS - $pass checks"
