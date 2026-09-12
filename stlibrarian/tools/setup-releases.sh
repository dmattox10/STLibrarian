#!/usr/bin/env bash
# One-time setup for publishing builds, and a check you can re-run any time.
#
# The source repository is private, so its release assets need a token to
# download and a download button on a website cannot supply one. Builds
# therefore go to a separate public repository. This wires that up and, more
# usefully, proves the token actually works -- otherwise the first time you
# find out is when a tagged release fails at midnight.
set -euo pipefail

SOURCE_REPO="${SOURCE_REPO:-dmattox10/STLibrarian}"
RELEASES_REPO="${RELEASES_REPO:-dmattox10/stlibrarian-releases}"
SECRET_NAME="RELEASES_TOKEN"

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m    %s\n' "$*"; }
bad()  { printf '  \033[31mno\033[0m    %s\n' "$*"; }

say "Checking the pieces"

command -v gh >/dev/null || { bad "the GitHub CLI is not installed"; exit 1; }
gh auth status >/dev/null 2>&1 || { bad "gh is not logged in -- run: gh auth login"; exit 1; }
ok "gh is logged in as $(gh api user -q .login)"

if gh repo view "$RELEASES_REPO" >/dev/null 2>&1; then
  VIS="$(gh repo view "$RELEASES_REPO" --json visibility -q .visibility)"
  if [ "$VIS" = "PUBLIC" ]; then
    ok "$RELEASES_REPO exists and is public"
  else
    bad "$RELEASES_REPO exists but is $VIS -- downloads would need a login"
    exit 1
  fi
else
  bad "$RELEASES_REPO does not exist. Create it with:"
  echo "        gh repo create $RELEASES_REPO --public"
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

# ---------------------------------------------------------------- the token --
#
# A fine-grained token scoped to the releases repository only. It cannot read
# the private source, which is the entire point of using one rather than a
# token that reaches everything you own.
if have_secret "$SECRET_NAME"; then
  ok "$SECRET_NAME is set on $SOURCE_REPO"
else
  bad "$SECRET_NAME is not set on $SOURCE_REPO"
  cat <<TEXT

  Make one here:

    https://github.com/settings/personal-access-tokens/new

    Resource owner       your own account
    Repository access    Only select repositories -> $RELEASES_REPO
    Permissions          Repository permissions -> Contents -> Read and write
    Expiration           whatever you are willing to renew

  Then install it:

    gh secret set $SECRET_NAME --repo $SOURCE_REPO

  and paste the token when it asks. Re-run this script afterwards and it will
  check the token can actually publish.

TEXT
  exit 1
fi

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

# --------------------------------------------------------- prove it works ----
#
# A secret that exists is not a secret that works. The only way to know is to
# use it, so this cuts a throwaway draft release and deletes it again. A draft
# is visible to nobody and notifies nobody.
say "Proving the token can publish"

if [ -z "${RELEASES_TOKEN:-}" ]; then
  cat <<'TEXT'
  Skipped. To check the token end to end, run this script once with the token
  in the environment (running it this way stores the token nowhere):

    RELEASES_TOKEN=github_pat_... tools/setup-releases.sh

TEXT
  exit 0
fi

TAG="setup-check-$$"
if GH_TOKEN="$RELEASES_TOKEN" gh release create "$TAG" \
     --repo "$RELEASES_REPO" --draft --title "setup check" --notes "throwaway" >/dev/null 2>&1; then
  ok "the token created a draft release"
  if GH_TOKEN="$RELEASES_TOKEN" gh release delete "$TAG" --repo "$RELEASES_REPO" --yes >/dev/null 2>&1; then
    ok "and cleaned it up again"
  else
    bad "could not delete the draft $TAG -- remove it by hand"
  fi
else
  bad "the token could NOT create a release in $RELEASES_REPO"
  echo "        Check it is scoped to that repo with Contents: read and write."
  exit 1
fi

say "Ready. Tag a version to publish:"
cat <<'TEXT'
  # bump the version in package.json, src-tauri/Cargo.toml and tauri.conf.json
  git tag v0.2.0 && git push origin v0.2.0
TEXT
