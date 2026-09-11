# Installing STLibrarian Pro on a server or NAS

The desktop app is a bundle you double-click. This is the equivalent for the
server: pick one of the three routes below, none of which need a compiler on
the target machine.

| Route | For | Effort |
| --- | --- | --- |
| [Docker](#docker) | Synology, QNAP, unRAID, TrueNAS, any Linux box | one compose file |
| [A prebuilt binary](#a-prebuilt-binary) | a plain Linux server, no Docker | download, run |
| [From source](#from-source) | developing on it | two toolchains |

Whichever you use, the first start prints an administrator password **once**.
Save it before you close the window.

---

## Docker

The recommended route, and the only one that is genuinely one step on a NAS.

```bash
git clone https://github.com/dmattox10/STLibrarian.git
cd STLibrarian/stlibrarian
# point the two volumes at your own share
$EDITOR docker-compose.yml
docker compose up -d
docker compose logs stlibrarian     # <- the admin password, printed once
```

Then open `http://your-nas:3000`.

### The two volumes

```yaml
volumes:
  - /volume1/3dprints:/library   # your models
  - ./data:/data                 # stl.db and thumbnails -- back this up
```

`/library` is where your models already live; nothing is copied into the
container. `/data` is the library itself, and is the only thing that cannot be
regenerated. Everything the app knows -- tags, packages, collections, print
history -- is `stl.db` in there.

`/library` is mounted read-write because **Organise** rearranges files on disk,
which is the one thing the server does that the desktop app will not. If you
never plan to use it, make the mount `:ro`; Organise will then fail with a
permissions error rather than appearing to work.

### Permissions

The container runs as root by default, because the right answer depends on who
owns your share and guessing wrong produces an empty library with the reason
buried in a log. Once you know the owner:

```bash
stat -c '%u:%g' /volume1/3dprints    # e.g. 1026:100 on a Synology
```

then set `user: "1026:100"` in `docker-compose.yml` and recreate the container.

### Synology

Container Manager → Project → Create → upload this `docker-compose.yml`. Set
the library path to your real share (`/volume1/...`, which you can read off
File Station). Read the project's log for the admin password.

### unRAID

Community Applications does not have a template yet. Until it does: Docker tab
→ Add Container → Advanced view, image `ghcr.io/dmattox10/stlibrarian:latest`,
port `3000`, and two paths, `/library` and `/data`.

### Upgrading

```bash
docker compose pull && docker compose up -d
```

The database migrates itself on start. Back up `data/` first -- see
[Backups](#backups).

---

## A prebuilt binary

One file, no runtime, no Docker. It needs glibc 2.36 or newer (Debian 12,
Ubuntu 22.04+, or anything from the last couple of years).

```bash
curl -fsSL -o stlibrarian-server \
  https://github.com/dmattox10/STLibrarian/releases/latest/download/stlibrarian-server-linux-x86_64
curl -fsSL -o stlibrarian-web.tar.gz \
  https://github.com/dmattox10/STLibrarian/releases/latest/download/stlibrarian-web.tar.gz

chmod +x stlibrarian-server
sudo install -m755 stlibrarian-server /usr/local/bin/
sudo mkdir -p /var/lib/stlibrarian /usr/share/stlibrarian
sudo tar xzf stlibrarian-web.tar.gz -C /usr/share/stlibrarian

sudo STLIBRARIAN_DATA_DIR=/var/lib/stlibrarian \
     STLIBRARIAN_WEB_ROOT=/usr/share/stlibrarian/web \
     stlibrarian-server
```

To keep it running, install the unit in
[`packaging/stlibrarian-server.service`](../packaging/stlibrarian-server.service):

```bash
sudo cp packaging/stlibrarian-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now stlibrarian-server
sudo journalctl -u stlibrarian-server | grep -A3 'first run'   # the password
```

---

## From source

What you need if you are working on it.

```bash
yarn install && yarn build            # the web front end -> dist/
cd src-tauri
cargo build --release --no-default-features --features server \
      --bin stlibrarian-server
```

`--no-default-features` matters: the default feature set is the desktop app,
which links Tauri and therefore GTK and webkit2gtk. A headless server has
none of those and does not need them.

---

## Settings

Every one of these is an environment variable, and every one has a working
default.

| Variable | Default | What it does |
| --- | --- | --- |
| `STLIBRARIAN_DATA_DIR` | OS data dir | Where `stl.db` and `thumbnails/` live |
| `STLIBRARIAN_WEB_ROOT` | `../dist` | The built web UI to serve |
| `STLIBRARIAN_BIND` | `0.0.0.0:3000` | Address and port |
| `RUST_LOG` | `info` | `debug` when something is wrong |

---

## Putting it behind a domain

The server speaks plain HTTP and does not terminate TLS. Behind a reverse
proxy, two settings matter more than the rest:

- **No upload or response size limit.** Models run to hundreds of megabytes and
  are streamed; a proxy buffering them into memory or capping the body will
  break downloads for exactly the largest files people care about.
- **A generous read timeout.** Scanning a large share and rendering previews
  are slow on purpose. 300s is a reasonable ceiling.

For nginx:

```nginx
location / {
    proxy_pass http://stlibrarian:3000;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;

    client_max_body_size 0;      # models are large
    proxy_request_buffering off; # stream, do not spool to disk
    proxy_buffering off;
    proxy_read_timeout 300s;
}
```

In Nginx Proxy Manager, those go in the proxy host's **Advanced** tab.

---

## Backups

Back up the `/data` volume. `stl.db` is the library; `thumbnails/` is redrawn
from your models if it is missing, so skipping it costs nothing but time.

Stop the container first, or copy `stl.db-wal` and `stl.db-shm` alongside
`stl.db` -- those hold writes not yet folded into the main file, and a copy
without them can be a moment stale.

---

## Losing the admin password

It is printed once, on first start, and stored only as a hash. If you lose it
and no other admin account exists, the recovery is to remove the accounts and
let the server mint a new one; the library itself is untouched.

```bash
docker compose stop stlibrarian
sqlite3 data/stl.db 'DELETE FROM users;'
docker compose start stlibrarian
docker compose logs --tail=20 stlibrarian
```
