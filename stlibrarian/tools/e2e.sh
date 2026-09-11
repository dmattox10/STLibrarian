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
check "  and the mesh is closed"  "$(api "$BASE/api/models/$CUBE_ID" | python3 -c 'import sys,json;print(json.load(sys.stdin)["watertight"])')" "True"
# The fixtures are open shells, so they must not claim otherwise.
check "an open shell is flagged"  "$(api "$BASE/api/models?search=widget" | python3 -c 'import sys,json;print(json.load(sys.stdin)[0]["watertight"])')" "False"

echo "== server-side rendering =="
# No PUT first: the server has to rasterise this itself, which is the whole
# point -- a headless NAS has no browser to make previews for it.
CUBE_HASH="$(api "$BASE/api/models?search=cube" | jfirst hash)"
check "rendered on demand"      "$(api_code "$BASE/api/thumb/$CUBE_HASH")" 200
check "  is a PNG"              "$(api "$BASE/api/thumb/$CUBE_HASH" | head -c 4 | tail -c 3)" "PNG"
check "  cached to disk"        "$(ls "$WORK/data/thumbnails" | grep -c "$CUBE_HASH")" 1


echo "== packages, duplicates, provenance =="
# A byte-identical copy of one file, and a folder holding two parts.
mkdir -p "$WORK/models/Gods/Throk_130mm/Throk_130mm_Unsupported" "$WORK/models/dupes"
cp "$WORK/models/widget.stl" "$WORK/models/dupes/widget.stl"
cp "$WORK/models/nested/BRACKET.STL" "$WORK/models/Gods/Throk_130mm/Throk_130mm_Unsupported/body.stl"
cp "$WORK/models/cube.stl" "$WORK/models/Gods/Throk_130mm/Throk_130mm_Unsupported/arm.stl"
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null

# Targeted rather than positional: the fixture makes several duplicate pairs
# and the list is ordered by how much space each wastes.
dupe_copies() { python3 -c 'import sys,json;print(next(g["copies"] for g in json.load(sys.stdin) if g["name"]==sys.argv[1]))' "$1"; }
sug_named()   { python3 -c 'import sys,json;print(sum(1 for s in json.load(sys.stdin) if s["name"]==sys.argv[1]))' "$1"; }
sug_ids()     { python3 -c 'import sys,json;print(json.dumps(next(s["model_ids"] for s in json.load(sys.stdin) if s["name"]==sys.argv[1]),separators=(",",":")))' "$1"; }
model_field() { python3 -c 'import sys,json;print(json.load(sys.stdin)[0][sys.argv[1]])' "$1"; }
has_creator() { python3 -c 'import sys,json;print(sum(1 for c in json.load(sys.stdin) if c["name"]==sys.argv[1]))' "$1"; }

check "the copied file is a duplicate" "$(api "$BASE/api/duplicates" | dupe_copies widget)" 2
check "  originals are not"            "$(api "$BASE/api/duplicates" | python3 -c 'import sys,json;print(sum(1 for g in json.load(sys.stdin) if g["copies"]<2))')" 0

check "suggests the parts folder"      "$(api "$BASE/api/packages/suggestions" | sug_named "Throk 130mm")" 1
check "  with both parts"              "$(api "$BASE/api/packages/suggestions" | python3 -c 'import sys,json;print(next(s["parts"] for s in json.load(sys.stdin) if s["name"]=="Throk 130mm"))')" 2
check "nothing packaged yet"           "$(api "$BASE/api/packages" | jlen)" 0

SUG_IDS="$(api "$BASE/api/packages/suggestions" | sug_ids "Throk 130mm")"
PKG_BODY="$(printf '{"name":"Throk 130mm","root":"/x","model_ids":%s}' "$SUG_IDS")"
PKG_ID="$(post "$BASE/api/packages" "$PKG_BODY" | jfield id)"
check "package created with its parts" "$(api "$BASE/api/packages" | jfirst parts)" 2
check "  filter by package"            "$(api "$BASE/api/models?package=$PKG_ID" | jlen)" 2
check "  that group stops being suggested" "$(api "$BASE/api/packages/suggestions" | sug_named "Throk 130mm")" 0

check "creator guessed from the path"  "$(api "$BASE/api/models?search=body" | model_field creator)" "Gods"
check "  and listed"                   "$(api "$BASE/api/creators" | has_creator Gods)" 1
WIDGET_ID="$(api "$BASE/api/models?search=widget" | jfirst id)"
PROV='{"creator":"Archvillain Games","source":"Patreon Jan 2021"}'
check "provenance is editable"         "$(post_code "$BASE/api/models/$WIDGET_ID/provenance" "$PROV")" 204
check "  and reads back"               "$(api "$BASE/api/models/$WIDGET_ID" | jfield creator)" "Archvillain Games"

check "deleting a package keeps files"  "$(api_code -X DELETE "$BASE/api/packages/$PKG_ID")" 204
check "  the files are still indexed"   "$(api "$BASE/api/models?search=body" | jlen)" 1

echo "== creator renders =="
# A folder holding a model and the creator's promo shot beside it.
mkdir -p "$WORK/models/Creator/Hero"
# A distinct cube: identical content would share a hash with an earlier
# fixture, and previews are content-addressed, so the two would share a
# cached picture and the assertions below would be testing nothing.
python3 - "$WORK/models/Creator/Hero/hero.stl" <<'PYHERO'
import struct, sys, pathlib
s = 17.0
p = [[0,0,0],[s,0,0],[s,s,0],[0,s,0],[0,0,s],[s,0,s],[s,s,s],[0,s,s]]
faces = [(0,1,2),(0,2,3),(4,6,5),(4,7,6),(0,5,1),(0,4,5),
         (1,6,2),(1,5,6),(2,7,3),(2,6,7),(3,4,0),(3,7,4)]
out = bytearray(b"\0"*80) + struct.pack("<I", len(faces))
for a,b,c in faces:
    out += b"\0"*12
    for v in (p[a],p[b],p[c]): out += struct.pack("<3f", *v)
    out += b"\0\0"
pathlib.Path(sys.argv[1]).write_bytes(bytes(out))
PYHERO
python3 - "$WORK/models/Creator/Hero/hero.jpg" <<'PYIMG'
import zlib, struct, sys, pathlib
# A real PNG (written with a .jpg name on purpose: the decoder must go by
# content, not by extension, because creators mislabel files constantly).
w = h = 240
rows = b"".join(b"\x00" + bytes([(x * 255) // w, (y * 255) // h, 128] * 1) * w
                for y in range(h) for x in [0])
raw = b"".join(b"\x00" + b"".join(bytes([(x*255)//w, (y*255)//h, 128]) for x in range(w)) for y in range(h))
def chunk(tag, data):
    c = tag + data
    return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)
png = (b"\x89PNG\r\n\x1a\n"
       + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(raw))
       + chunk(b"IEND", b""))
pathlib.Path(sys.argv[1]).write_bytes(png)
PYIMG
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null

HERO_ID="$(api "$BASE/api/models?search=hero" | jfirst id)"
HERO_HASH="$(api "$BASE/api/models?search=hero" | jfirst hash)"
check "render matched to the model" "$(api "$BASE/api/models/$HERO_ID" | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin)["render"] else "no")')" "yes"
check "  offered as a choice"       "$(api "$BASE/api/models/$HERO_ID/renders" | jlen)" 1
check "  preview serves"            "$(api_code "$BASE/api/thumb/$HERO_HASH")" 200
check "  and is a PNG"              "$(api "$BASE/api/thumb/$HERO_HASH" | head -c 4 | tail -c 3)" "PNG"
RENDER_SIZE="$(api "$BASE/api/thumb/$HERO_HASH" | wc -c | tr -d ' ')"

check "can revert to the mesh"      "$(post_code "$BASE/api/models/$HERO_ID/render" '{"image_id":null}')" 204
check "  render is cleared"         "$(api "$BASE/api/models/$HERO_ID" | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin)["render"] else "no")')" "no"
MESH_SIZE="$(api "$BASE/api/thumb/$HERO_HASH" | wc -c | tr -d ' ')"
# Compared in bash: interpolating a shell variable into a single-quoted
# python -c inside a command substitution is the quoting trap this script
# already has a warning about at the top.
check "  preview was rebuilt"       "$([ "$MESH_SIZE" != "$RENDER_SIZE" ] && echo changed || echo same)" "changed"

# Only a model with no render of its own falls back to the rasteriser.
check "models without a render still draw" "$(api_code "$BASE/api/thumb/$CUBE_HASH")" 200

echo "== print records =="
PRINT='{"outcome":"success","printer":"Mars 4 Ultra","material":"grey resin","settings":"0.05mm","notes":"clean"}'
FAILED='{"outcome":"failed","printer":"Mars 4 Ultra","notes":"peeled off the plate"}'
check "nothing printed yet"      "$(api "$BASE/api/models/$HERO_ID" | jfield print_count)" 0
check "logging a print"          "$(post_code "$BASE/api/models/$HERO_ID/prints" "$PRINT")" 200
check "  counted on the model"   "$(api "$BASE/api/models/$HERO_ID" | jfield print_count)" 1
check "  history readable"       "$(api "$BASE/api/models/$HERO_ID/prints" | jlen)" 1
check "  printer kept"           "$(api "$BASE/api/models/$HERO_ID/prints" | jfirst printer)" "Mars 4 Ultra"

check "a failure is still logged" "$(post_code "$BASE/api/models/$HERO_ID/prints" "$FAILED")" 200
check "  both attempts counted"   "$(api "$BASE/api/models/$HERO_ID" | jfield print_count)" 2
check "nonsense outcome refused"  "$(post_code "$BASE/api/models/$HERO_ID/prints" '{"outcome":"maybe"}')" 400

check "filter: printed"          "$(api "$BASE/api/models?printed=true" | jlen)" 1
NEVER="$(api "$BASE/api/models?printed=false" | jlen)"
check "filter: never printed"    "$([ "$NEVER" -gt 0 ] && echo some || echo none)" "some"
check "  the two are disjoint"   "$(api "$BASE/api/models?printed=false" | python3 -c 'import sys,json;print(sum(1 for m in json.load(sys.stdin) if m["print_count"]>0))')" 0

PRINT_ID="$(api "$BASE/api/models/$HERO_ID/prints" | jfirst id)"
check "deleting one attempt"     "$(api_code -X DELETE "$BASE/api/prints/$PRINT_ID")" 204
check "  the other survives"     "$(api "$BASE/api/models/$HERO_ID" | jfield print_count)" 1

echo "== variants and other formats =="
mkdir -p "$WORK/models/Pack/Presupported"
cp "$WORK/models/cube.stl" "$WORK/models/Pack/Presupported/knight.stl"
cat > "$WORK/models/Pack/tetra.obj" <<'PYOBJ'
v 0 0 0
v 2 0 0
v 0 3 0
v 0 0 4
f 1 2 3
f 1 2 4
f 2 3 4
f 1 3 4
PYOBJ
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null

check "obj indexed"              "$(api "$BASE/api/models?search=tetra" | jlen)" 1
check "  triangles counted"      "$(api "$BASE/api/models?search=tetra" | jfirst triangles)" 4
check "  bounds measured"        "$(api "$BASE/api/models?search=tetra" | python3 -c 'import sys,json;m=json.load(sys.stdin)[0];print("%gx%gx%g" % (m["dim_x"],m["dim_y"],m["dim_z"]))')" "2x3x4"
check "  and it previews"        "$(api_code "$BASE/api/thumb/$(api "$BASE/api/models?search=tetra" | jfirst hash)")" 200

check "presupported detected"    "$(api "$BASE/api/models?search=knight" | jfirst variant)" "supported"
check "unsupported detected"     "$(api "$BASE/api/models?search=body" | jfirst variant)" "unsupported"
check "filter by variant"        "$(api "$BASE/api/models?variant=supported" | jlen)" 1
check "  the two do not overlap" "$(api "$BASE/api/models?variant=unsupported" | python3 -c 'import sys,json;print(sum(1 for m in json.load(sys.stdin) if m["variant"]!="unsupported"))')" 0

python3 - "$WORK/models/Pack/widget.3mf" <<'PY3MF'
import sys, zipfile, pathlib
xml = """<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter">
 <resources><object id="1" type="model"><mesh>
  <vertices>
   <vertex x="0" y="0" z="0"/><vertex x="7" y="0" z="0"/>
   <vertex x="0" y="8" z="0"/><vertex x="0" y="0" z="9"/>
  </vertices>
  <triangles>
   <triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="3"/>
   <triangle v1="1" v2="2" v3="3"/><triangle v1="0" v2="2" v3="3"/>
  </triangles>
 </mesh></object></resources>
 <build><item objectid="1"/></build>
</model>"""
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("3D/3dmodel.model", xml)
PY3MF
post "$BASE/api/scan" "$SCAN_BODY" >/dev/null
THREEMF="$(api "$BASE/api/models" | python3 -c 'import sys,json;print(sum(1 for m in json.load(sys.stdin) if m["path"].endswith(".3mf")))')"
check "  the 3mf is in the library" "$THREEMF" 1
check "  triangles counted"      "$(api "$BASE/api/models?sort=name" | python3 -c 'import sys,json;print(next(m["triangles"] for m in json.load(sys.stdin) if m["path"].endswith(".3mf")))')" 4
check "  bounds measured"        "$(api "$BASE/api/models" | python3 -c 'import sys,json;m=next(m for m in json.load(sys.stdin) if m["path"].endswith(".3mf"));print("%gx%gx%g" % (m["dim_x"],m["dim_y"],m["dim_z"]))')" "7x8x9"
check "  and it previews"        "$(api_code "$BASE/api/thumb/$(api "$BASE/api/models" | python3 -c 'import sys,json;print(next(m["hash"] for m in json.load(sys.stdin) if m["path"].endswith(".3mf")))')")" 200

echo "== organising files on disk (Pro) =="
# Deliberately last: this is the only thing that moves the fixtures, so every
# assertion above still describes where the files started.
ORG="$WORK/organized"
ORG_BODY="$(printf '{"root":"%s"}' "$ORG")"

PLAN="$(post "$BASE/api/organize/plan" "$ORG_BODY")"
plan_count() { python3 -c 'import sys,json;print(sum(1 for m in json.load(sys.stdin) if m["action"]==sys.argv[1]))' "$1"; }
check "plan proposes moves"      "$(printf '%s' "$PLAN" | plan_count move | python3 -c 'import sys;print("yes" if int(sys.stdin.read())>0 else "no")')" "yes"
check "  nothing in place yet"   "$(printf '%s' "$PLAN" | plan_count inplace)" 0
check "  no destination escapes" "$(printf '%s' "$PLAN" | python3 -c 'import sys,json,os;print(sum(1 for m in json.load(sys.stdin) if not os.path.abspath(m["to"]).startswith(os.path.abspath(sys.argv[1]))))' "$ORG")" 0
check "  planning moved nothing" "$([ -d "$ORG" ] && echo exists || echo absent)" "absent"

BEFORE="$(api "$BASE/api/models" | jlen)"
REPORT="$(post "$BASE/api/organize/apply" "$ORG_BODY")"
check "apply moved files"        "$(printf '%s' "$REPORT" | python3 -c 'import sys,json;print("yes" if json.load(sys.stdin)["moved"]>0 else "no")')" "yes"
check "  nothing failed"         "$(printf '%s' "$REPORT" | jfield failed)" 0
check "  library still complete" "$(api "$BASE/api/models" | jlen)" "$BEFORE"
check "  files really moved"     "$([ -d "$ORG" ] && echo yes || echo no)" "yes"
check "  rows point at the new paths" "$(api "$BASE/api/models" | python3 -c 'import sys,json;print(sum(1 for m in json.load(sys.stdin) if not m["path"].startswith(sys.argv[1])))' "$ORG")" 0
check "  every file exists where the row says" "$(api "$BASE/api/models" | python3 -c 'import sys,json,os;print(sum(1 for m in json.load(sys.stdin) if not os.path.isfile(m["path"])))')" 0

# Parts of one package share a folder.
check "package parts land together" "$(api "$BASE/api/models" | python3 -c '
import sys, json, os
by = {}
for m in json.load(sys.stdin):
    if m["package_id"]:
        by.setdefault(m["package_id"], set()).add(os.path.dirname(m["path"]))
print(sum(1 for dirs in by.values() if len(dirs) > 1))')" 0

REPLAN="$(post "$BASE/api/organize/plan" "$ORG_BODY")"
check "re-planning finds nothing to do" "$(printf '%s' "$REPLAN" | plan_count move)" 0
check "  everything is in place"        "$(printf '%s' "$REPLAN" | plan_count inplace | python3 -c 'import sys;print("yes" if int(sys.stdin.read())>0 else "no")')" "yes"
check "applying again is a no-op"       "$(post "$BASE/api/organize/apply" "$ORG_BODY" | jfield moved)" 0

echo
if [ "$fail" -gt 0 ]; then
  echo "RESULT FAIL - $pass passed, $fail failed"
  exit 1
fi
echo "RESULT PASS - $pass checks"
