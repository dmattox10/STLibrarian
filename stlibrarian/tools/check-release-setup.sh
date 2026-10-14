#!/usr/bin/env bash
# What a tagged release would actually do right now, before you tag one.
#
# There used to be far more here. While the source repository was private its
# release assets needed a token to download, a download button on a website
# cannot supply one, so builds crossed to a separate public repository behind a
# fine-grained PAT and most of this script existed to prove that PAT worked.
# The source is public and MIT licensed now, releases are cut on this
# repository by the built-in GITHUB_TOKEN, and there is nothing left to wire
# up. What is still worth checking is signing, because it is optional and its
# absence is silent until someone downloads a build.
set -euo pipefail

SOURCE_REPO="${SOURCE_REPO:-dmattox10/STLibrarian}"

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m    %s\n' "$*"; }
bad()  { printf '  \033[31mno\033[0m    %s\n' "$*"; }

say "Checking the pieces"

command -v gh >/dev/null || { bad "the GitHub CLI is not installed"; exit 1; }
gh auth status >/dev/null 2>&1 || { bad "gh is not logged in -- run: gh auth login"; exit 1; }
ok "gh is logged in as $(gh api user -q .login)"

# The one thing the whole delivery route now rests on. If this repository ever
# goes private again, every download link on the site breaks at once and this
# is the line that says so.
VIS="$(gh repo view "$SOURCE_REPO" --json visibility -q .visibility 2>/dev/null || true)"
if [ "$VIS" = "PUBLIC" ]; then
  ok "$SOURCE_REPO is public, so release assets download without an account"
elif [ -n "$VIS" ]; then
  bad "$SOURCE_REPO is $VIS -- the download buttons on the site cannot fetch from it"
  echo "        Make it public again, or put the separate releases repo back."
  exit 1
else
  bad "could not read $SOURCE_REPO"
  exit 1
fi

# Fetched once, not once per key. Asking six times in a row got throttled and
# the failures were invisible behind 2>/dev/null, so a key that was present
# reported missing -- a different one each run, which is the tell. A lookup
# that cannot answer must say so rather than answer "no".
SECRET_LIST="$(gh secret list --repo "$SOURCE_REPO" 2>/dev/null || true)"
if [ -z "$SECRET_LIST" ]; then
  bad "could not read the secret list for $SOURCE_REPO"
  echo "        Nothing below about signing can be trusted; try again in a moment."
  exit 1
fi
have_secret() { printf '%s\n' "$SECRET_LIST" | grep -q "^$1[[:space:]]"; }

# ------------------------------------------------------------- signing -------
#
# Optional, and the pipeline works without it. But an unsigned build is refused
# by Gatekeeper on download ("STLibrarian is damaged and can't be opened") and
# warned about by SmartScreen, so a public download page wants these.
say "Signing"

MAC_NEEDED="APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID"
MAC_MISSING=""
for k in $MAC_NEEDED; do have_secret "$k" || MAC_MISSING="$MAC_MISSING $k"; done
if [ -z "$MAC_MISSING" ]; then
  ok "macOS builds will be signed and notarized"
else
  bad "macOS builds will be UNSIGNED. Gatekeeper refuses these on download."
  echo "        missing:$MAC_MISSING"
  echo "        Run tools/setup-signing-macos.sh -- it does the whole thing."
fi

# Windows is deliberately not signed. An OV certificate would not silence
# SmartScreen -- that needs EV plus reputation built over time -- so it is a
# few hundred a year that changes nothing the user sees. The download page
# says so plainly instead.
ok "Windows builds are unsigned by choice; the site says so"
ok "Linux builds need no signing"

# ------------------------------------------------------------- leftovers -----
#
# The old cross-repo PAT. Nothing reads it any more, and a live token with
# write access to a repository is not something to leave lying around because
# it stopped being load-bearing.
if have_secret "RELEASES_TOKEN"; then
  say "Leftovers"
  bad "RELEASES_TOKEN is still set and is now read by nothing. Delete it:"
  echo "        gh secret delete RELEASES_TOKEN --repo $SOURCE_REPO"
  echo "        and revoke it at https://github.com/settings/personal-access-tokens"
fi

say "Ready. Tag a version to publish:"
cat <<'TEXT'
  # bump the version in package.json, src-tauri/Cargo.toml and tauri.conf.json
  git tag v0.2.0 && git push origin v0.2.0
TEXT
