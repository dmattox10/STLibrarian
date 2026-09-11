#!/usr/bin/env bash
# Installs the six secrets CI needs to sign and notarize the macOS builds.
#
#   tools/setup-signing-macos.sh ~/Desktop/DeveloperID.p12
#
# It takes a .p12 you exported rather than exporting one itself, deliberately.
# `security export -t identities` takes EVERY identity in the keychain, so on a
# machine that also has an "Apple Development" certificate it would ship that
# private key to GitHub too. Exporting one certificate from Keychain Access
# sends exactly the key that is needed and nothing else.
set -euo pipefail

REPO="${REPO:-dmattox10/STLibrarian}"
P12="${1:-}"

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m    %s\n' "$*"; }
bad()  { printf '  \033[31mno\033[0m    %s\n' "$*"; }

say "Looking for a distribution certificate"

# "Apple Development" signs builds for your own machines; a download signed
# with it is still refused by Gatekeeper. Distribution outside the App Store
# needs "Developer ID Application", which is a separate certificate.
IDENTITIES="$(security find-identity -v -p codesigning 2>/dev/null || true)"
IDENTITY="$(printf '%s\n' "$IDENTITIES" | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"

if [ -z "$IDENTITY" ]; then
  bad "no Developer ID Application certificate in your keychain"
  printf '%s\n' "$IDENTITIES" | sed 's/^/        /'
  cat <<'TEXT'

  Make one: Xcode -> Settings -> Accounts -> your Apple ID -> Manage
  Certificates -> the + at the bottom left -> Developer ID Application.
  Only a team's Account Holder or Admin may create one.
TEXT
  exit 1
fi
ok "$IDENTITY"

TEAM_ID="$(printf '%s' "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p')"
[ -n "$TEAM_ID" ] || { bad "could not read the team ID out of that identity"; exit 1; }
ok "team $TEAM_ID"

EXPIRY="$(security find-certificate -c "$IDENTITY" -p 2>/dev/null | openssl x509 -noout -enddate 2>/dev/null | sed 's/notAfter=//')"
[ -n "$EXPIRY" ] && ok "valid until $EXPIRY"

if [ -z "$P12" ]; then
  cat <<TEXT

  Now export just that certificate, so only its key leaves this machine:

    Keychain Access -> login -> My Certificates
      -> right-click "$IDENTITY"
      -> Export... -> Personal Information Exchange (.p12)

  It will ask you to invent an export password. Anything will do; it protects
  the file only between here and GitHub.

  Then run:

    tools/setup-signing-macos.sh /path/to/that.p12

TEXT
  exit 1
fi

[ -f "$P12" ] || { bad "no such file: $P12"; exit 1; }

say "Checking the export"
printf '  Its export password: '
read -r -s P12_PASSWORD
printf '\n'

pem() { openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nokeys -clcerts -legacy 2>/dev/null \
        || openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nokeys -clcerts 2>/dev/null; }

CERTS="$(pem || true)"
if [ -z "$CERTS" ]; then
  bad "that password does not open the file, or it holds no certificate"
  exit 1
fi
ok "the password opens it"

# Exactly one, and the right one. Catching a stray key here is the whole
# reason this step exists: a .p12 holding two identities uploads a private
# key that has no business in CI.
COUNT="$(printf '%s\n' "$CERTS" | grep -c 'BEGIN CERTIFICATE' || true)"
SUBJECTS="$(printf '%s\n' "$CERTS" | openssl x509 -noout -subject 2>/dev/null || true)"

if [ "$COUNT" -ne 1 ]; then
  bad "that file holds $COUNT certificates; it should hold exactly one"
  echo "        Export the single Developer ID Application certificate rather"
  echo "        than a selection, so no other private key is uploaded."
  exit 1
fi
case "$SUBJECTS" in
  *"Developer ID Application"*) ok "one certificate, and it is the Developer ID one" ;;
  *) bad "that is not a Developer ID Application certificate:"
     printf '        %s\n' "$SUBJECTS"
     exit 1 ;;
esac

# And it must carry the private key, or CI signs nothing.
if ! openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nocerts -noout -legacy >/dev/null 2>&1 \
   && ! openssl pkcs12 -in "$P12" -passin "pass:$P12_PASSWORD" -nocerts -noout >/dev/null 2>&1; then
  bad "no private key in that file; export the certificate, not just its public half"
  exit 1
fi
ok "it carries the private key"

# The four that come from the certificate are known now. The two Apple
# account ones are not derivable from anything on this machine, so they may
# arrive in the environment, or be typed, or be left for a later run -- the
# four go up either way rather than making you start over.
say "Uploading what comes from the certificate"
# Values go in over stdin so none of them lands in the process list.
set_secret() { printf '%s' "$2" | gh secret set "$1" --repo "$REPO" >/dev/null; ok "$1"; }

base64 -i "$P12" | gh secret set APPLE_CERTIFICATE --repo "$REPO" >/dev/null
ok "APPLE_CERTIFICATE"
set_secret APPLE_CERTIFICATE_PASSWORD "$P12_PASSWORD"
set_secret APPLE_SIGNING_IDENTITY "$IDENTITY"
set_secret APPLE_TEAM_ID "$TEAM_ID"

say "Notarization"
if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ]; then
  set_secret APPLE_ID "$APPLE_ID"
  set_secret APPLE_PASSWORD "$APPLE_PASSWORD"
elif [ -t 0 ]; then
  cat <<'TEXT'
  Needs an app-specific password, never your Apple ID password. Make one at
  https://appleid.apple.com -> Sign-In and Security -> App-Specific Passwords.

TEXT
  printf '  Apple ID email: '
  read -r APPLE_ID
  printf '  App-specific password: '
  read -r -s APPLE_PASSWORD
  printf '\n'
  if [ -n "$APPLE_ID" ] && [ -n "$APPLE_PASSWORD" ]; then
    set_secret APPLE_ID "$APPLE_ID"
    set_secret APPLE_PASSWORD "$APPLE_PASSWORD"
  else
    bad "skipped; builds will be signed but not notarized"
  fi
else
  bad "APPLE_ID and APPLE_PASSWORD not set, and nothing to prompt"
  cat <<TEXT
        Signed but not notarized is still refused by Gatekeeper on download.
        Make an app-specific password at https://appleid.apple.com, then:

          gh secret set APPLE_ID --repo $REPO
          gh secret set APPLE_PASSWORD --repo $REPO

TEXT
fi

say "Where that leaves things"
"$(dirname "$0")/setup-releases.sh" || true

cat <<'TEXT'

  The .p12 on disk has done its job. Delete it:

    rm -P /path/to/that.p12

TEXT
