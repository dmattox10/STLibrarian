# Serving builds from your own box

Two different jobs get confused with each other, so decide which you want
before setting anything up.

| | Where builds live | Who it suits |
| --- | --- | --- |
| **GitHub Releases** | github.com | Nothing to run. Public, rate-limited, and the download page is GitHub's. |
| **Your own host** | your box, behind your domain | `dl.stlibrarian.com`, your own download page, and the desktop app updating itself from a URL you control. |

They are not exclusive. The sane arrangement is CI publishing to GitHub
Releases as the source of truth, and a copy pushed to your box for the download
links on the marketing page and for the updater.

---

## Your box, as it already is

You have Nginx Proxy Manager terminating TLS in front of containers on the
`proxiable` network. A download host fits that pattern exactly: a static nginx
container with no published port, reached only through the proxy.

### 1. The directory

```bash
mkdir -p /root/docker/stlibrarian-releases/releases
cp .../packaging/release-host/nginx.conf \
   /root/docker/stlibrarian-releases-nginx.conf
```

Layout, once a release or two has been published:

```
stlibrarian-releases/
├── latest.json                 <- what the desktop updater polls
├── latest -> releases/0.2.0    <- symlink, moved after a successful upload
└── releases/
    ├── 0.1.0/
    └── 0.2.0/
        ├── STLibrarian_0.2.0_aarch64.dmg
        ├── STLibrarian_0.2.0_x64-setup.exe
        ├── stlibrarian_0.2.0_amd64.AppImage
        ├── stlibrarian-server-linux-x86_64
        ├── stlibrarian-web.tar.gz
        └── SHA256SUMS
```

Every path under `releases/` contains a version, so those URLs are immutable
and cached for a year. `latest.json` is never cached -- a stale copy in some
proxy is a release that silently does not exist for whoever is behind it.

### 2. The container

Add the service in
[`packaging/release-host/docker-compose.snippet.yml`](../packaging/release-host/docker-compose.snippet.yml)
to `/root/docker/docker-compose.yml`, then:

```bash
cd /root/docker && docker compose up -d stlibrarian-dl
```

It joins `proxiable` and publishes no port, so nothing reaches it except
through the proxy.

### 3. The proxy host

In Nginx Proxy Manager → Hosts → Proxy Hosts → Add:

- **Domain:** `dl.stlibrarian.com`
- **Forward to:** `stlibrarian-dl` port `80`
- **Block common exploits:** on
- **SSL:** request a new certificate, force SSL, HTTP/2

Then, in the host's **Advanced** tab -- installers are large, and the defaults
are sized for web pages:

```nginx
client_max_body_size 0;
proxy_buffering off;
proxy_read_timeout 300s;
```

Point the DNS record at the box first, or the certificate request fails.

### 4. Publishing

```bash
RELEASE_HOST=root@your-box \
packaging/release-host/publish-release.sh 0.2.0 ./artifacts
```

Files go up first, `/latest` moves only after they all landed, and
`latest.json` is written last -- so a publish that fails part way leaves the
previous release current and complete rather than announcing something
half-uploaded.

---

## Letting the desktop app update itself

This is the part that makes a download host worth running: the app checks a URL
on your box and installs the new version itself, and nobody has to notice a
release happened.

**1. Generate a signing key**, once, and keep it somewhere you will not lose it:

```bash
cd stlibrarian && yarn tauri signer generate -w ~/.tauri/stlibrarian.key
```

The private key signs each build; the public key is compiled into the app, so
a build signed with anything else is refused. This is what stops your download
host from becoming a way to push arbitrary software onto your users' machines
if it is ever compromised.

**2. Configure it** in `src-tauri/tauri.conf.json`:

```jsonc
{
  "bundle": { "createUpdaterArtifacts": true },
  "plugins": {
    "updater": {
      "endpoints": ["https://dl.stlibrarian.com/latest.json"],
      "pubkey": "<the public key printed by signer generate>"
    }
  }
}
```

and add the `updater` plugin to the app (`@tauri-apps/plugin-updater` plus
`tauri-plugin-updater` in `Cargo.toml`, under the `desktop` feature).

**3. Write `latest.json`** next to each release:

```json
{
  "version": "0.2.0",
  "notes": "Sliced files, and the server remembering who printed what.",
  "pub_date": "2026-09-11T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<contents of the .sig file>",
      "url": "https://dl.stlibrarian.com/releases/0.2.0/STLibrarian_0.2.0_aarch64.app.tar.gz"
    },
    "darwin-x86_64": { "signature": "...", "url": "..." },
    "linux-x86_64":  { "signature": "...", "url": ".../stlibrarian_0.2.0_amd64.AppImage" },
    "windows-x86_64":{ "signature": "...", "url": ".../STLibrarian_0.2.0_x64-setup.nsis.zip" }
  }
}
```

The `signature` field is the *contents* of the `.sig` file tauri produced
beside each bundle, not a path to it. Check the artefact names against what
your build actually emitted before trusting the ones above -- they change with
Tauri versions, and a wrong URL here is an update that fails for one platform
only, which is the kind of thing that goes unnoticed for a month.

Keep the CI secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` set in the repository, and
[`.github/workflows/release.yml`](../../.github/workflows/release.yml) signs
every bundle it builds.

---

## The server image

The container image does not go on the download host. Push it to a registry --
`ghcr.io/dmattox10/stlibrarian`, which the release workflow already does -- so
a NAS can `docker compose pull`. A tarball someone has to `docker load` is not
an install path anybody follows.

Make the package public once (GitHub → Packages → stlibrarian → Package
settings → Change visibility), or every NAS owner needs a login before they can
pull.

---

## What not to do

- **Do not serve builds out of the git repository.** Binaries in git make every
  clone carry every version ever published, forever.
- **Do not put the download host behind the same container as the app.** The
  reason to have one is that it keeps working while you are restarting the
  other, which is exactly when someone is downloading a new build.
- **Do not skip the checksums.** `SHA256SUMS` costs one line in the publish
  script and is the only way anyone can tell a truncated download from a
  working one before they run it.
