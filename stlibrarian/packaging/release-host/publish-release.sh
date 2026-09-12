#!/usr/bin/env bash
#
# Upload a built release to the download host and make it the current one.
#
#   packaging/release-host/publish-release.sh 0.2.0 ./artifacts
#
# `artifacts` is a directory of finished installers and binaries -- whatever
# `tauri build` and the server build produced, collected in one place. The
# script does not build anything; it publishes what you hand it.
#
# Order matters and is deliberate: files go up first, checksums are written
# from what actually landed, and only then does /latest move. A publish that
# fails half way leaves the previous release current and complete.
set -euo pipefail

VERSION="${1:?usage: publish-release.sh <version> <artifact-dir>}"
SRC="${2:?usage: publish-release.sh <version> <artifact-dir>}"

# Where the release host keeps its files, and how to reach it.
HOST="${RELEASE_HOST:-root@your-box}"
ROOT="${RELEASE_ROOT:-/root/docker/stlibrarian-releases}"

[ -d "$SRC" ] || { echo "no such directory: $SRC" >&2; exit 1; }
[ -n "$(ls -A "$SRC")" ] || { echo "$SRC is empty" >&2; exit 1; }

echo "==> checksums"
( cd "$SRC" && sha256sum -- * > SHA256SUMS )

echo "==> uploading $VERSION"
# --partial so a dropped connection resumes; no --delete, because a release
# already published must never be removed by publishing a different one.
rsync -av --partial --progress \
  "$SRC/" "$HOST:$ROOT/releases/$VERSION/"

echo "==> pointing /latest at $VERSION"
# Replaced atomically: ln -sfn onto a temporary name and then mv, so a client
# fetching /latest/... never sees the link missing.
ssh "$HOST" "
  set -e
  cd '$ROOT'
  ln -sfn 'releases/$VERSION' .latest.new
  mv -T .latest.new latest
"

# latest.json is what the desktop app's updater polls. Written last: until it
# moves, no installed copy has been told the new version exists, so an upload
# that failed above is simply invisible rather than half-announced.
if [ -f "$SRC/latest.json" ]; then
  echo "==> announcing to the updater"
  rsync -av "$SRC/latest.json" "$HOST:$ROOT/latest.json"
else
  echo "==> no latest.json in $SRC; nothing announced to the updater"
fi

echo "==> published"
echo "    https://dl.stlibrarian.com/releases/$VERSION/"
