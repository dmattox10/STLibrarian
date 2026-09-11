#!/usr/bin/env bash
# Exports your Developer ID certificate and installs the six secrets CI needs
# to sign and notarize the macOS builds.
#
# Run it on the Mac whose keychain holds the certificate. It reads the
# keychain, writes a temporary .p12 into a directory only you can read, uploads
# the secrets, and shreds the temporary files on the way out.
set -euo pipefail

REPO="${REPO:-dmattox10/STLibrarian}"

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m    %s\n' "$*"; }
bad()  { printf '  \033[31mno\033[0m    %s\n' "$*"; }

WORK="$(mktemp -d)"
chmod 700 "$WORK"
cleanup() {
  # The .p12 is the private key. It does not get to linger in /tmp.
  if [ -d "$WORK" ]; then
    find "$WORK" -type f -exec rm -P {} + 2>/dev/null || true
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT INT TERM

say "Looking for a distribution certificate"

# The distinction that actually matters. "Apple Development" is for running on
# your own machines; an app signed with it is still refused by Gatekeeper when
# somebody downloads it. Distributing outside the App Store needs a
# "Developer ID Application" certificate, which is a different certificate you
# ask Apple for separately.
IDENTITIES="$(security find-identity -v -p codesigning 2>/dev/null || true)"
IDENTITY="$(printf '%s\n' "$IDENTITIES" | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"

if [ -z "$IDENTITY" ]; then
  bad "no Developer ID Application certificate in your keychain"
  echo
  printf '%s\n' "$IDENTITIES" | sed 's/^/        /'
  cat <<'TEXT'

  An "Apple Development" certificate is not the one. It signs builds for your
  own devices; a download signed with it is still refused by Gatekeeper.

  Make the right one, which takes about a minute:

    Xcode -> Settings -> Accounts -> (your Apple ID) -> Manage Certificates
      -> the + at the bottom left -> Developer ID Application

  If the + menu does not offer it, your account is not the team's Account
  Holder or Admin, which is the only role allowed to create one.

  Then run this again.
TEXT
  exit 1
fi
ok "found: $IDENTITY"

# The ten-character code in the parentheses.
TEAM_ID="$(printf '%s' "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p')"
[ -n "$TEAM_ID" ] || { bad "could not read the team ID out of that identity"; exit 1; }
ok "team ID: $TEAM_ID"

say "Exporting the certificate"
cat <<'TEXT'
  macOS will ask permission to export the private key, and may ask for your
  login password. That prompt is the export you just asked for.

  You will also be asked to invent an export password. It only protects the
  file between here and GitHub. Anything will do; it is stored as a secret and
  you will not need to remember it.

TEXT

P12="$WORK/cert.p12"
if ! security export -t identities -f pkcs12 -o "$P12"; then
  bad "export failed or was cancelled"
  exit 1
fi
[ -s "$P12" ] || { bad "the exported file is empty"; exit 1; }
ok "exported"

printf '\n  Type the same export password once more, so it can be stored: '
read -r -s P12_PASSWORD
printf '\n'
[ -n "$P12_PASSWORD" ] || { bad "an empty export password will not work"; exit 1; }

# Verify it actually opens with that password before uploading a secret that
# would otherwise fail silently in CI, weeks later.
if ! openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nokeys -clcerts -legacy >/dev/null 2>&1 \
   && ! openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nokeys -clcerts >/dev/null 2>&1; then
  bad "that password does not open the exported file"
  exit 1
fi
ok "the password opens it"

say "Notarization"
cat <<'TEXT'
  Notarization needs an app-specific password, never your Apple ID password.
  Make one at https://appleid.apple.com -> Sign-In and Security ->
  App-Specific Passwords. It looks like abcd-efgh-ijkl-mnop.

TEXT
printf '  Your Apple ID email: '
read -r APPLE_ID
printf '  App-specific password: '
read -r -s APPLE_PASSWORD
printf '\n'
[ -n "$APPLE_ID" ] && [ -n "$APPLE_PASSWORD" ] || { bad "both are required"; exit 1; }

say "Uploading secrets to $REPO"
base64 -i "$P12" > "$WORK/cert.b64"

set_secret() {
  # Values go in on stdin so none of them lands in the process list.
  printf '%s' "$2" | gh secret set "$1" --repo "$REPO" >/dev/null
  ok "$1"
}
gh secret set APPLE_CERTIFICATE --repo "$REPO" < "$WORK/cert.b64" >/dev/null
ok "APPLE_CERTIFICATE"
set_secret APPLE_CERTIFICATE_PASSWORD "$P12_PASSWORD"
set_secret APPLE_SIGNING_IDENTITY "$IDENTITY"
set_secret APPLE_ID "$APPLE_ID"
set_secret APPLE_PASSWORD "$APPLE_PASSWORD"
set_secret APPLE_TEAM_ID "$TEAM_ID"

say "Done. Checking how it looks now:"
"$(dirname "$0")/setup-releases.sh" || true
