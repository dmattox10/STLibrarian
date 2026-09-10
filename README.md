# STLibrarian

A media library for 3D printing. Point it at a folder of STLs and it indexes
everything inside, renders a preview of each model, and lets you tag, search and
browse the result — Plex, for the pile of models you have downloaded and never
found again.

It runs two ways from one codebase:

- **Desktop** — a Tauri app over your local disk. No accounts, no server.
- **Pro** — the same library on a NAS, served to a browser, with real users.

## Running it

```bash
yarn install
yarn tauri dev            # desktop app
```

```bash
yarn build                                       # build the web front end
cd src-tauri
cargo run --no-default-features --features server --bin stlibrarian-server
```

The server prints a generated admin password to its console on first run. It is
shown once and never stored in plaintext, so save it then.

| Variable | Default | Meaning |
| --- | --- | --- |
| `STLIBRARIAN_DATA_DIR` | platform data dir | Where `stl.db` and previews live |
| `STLIBRARIAN_BIND` | `0.0.0.0:3000` | Server listen address |
| `STLIBRARIAN_WEB_ROOT` | `../dist` | Built front end to serve |

## Tests

```bash
cd src-tauri && cargo test --no-default-features --lib   # core: 21 tests
tools/e2e.sh                                             # server + API: 21 checks
```

`tools/e2e.sh` builds everything, starts a server on a scratch port against a
scratch data directory, and exercises the API the way a client does. It touches
nothing you own.

## How it is put together

```
src/
  data/          the only code that knows which backend is running
    adapter.js     picks one at startup
    tauri.js       desktop, over Tauri IPC
    electron.js    desktop, over a preload bridge
    http.js        Pro, over HTTP
  theme/         Nord palette and the game-HUD control set
  components/    the UI. None of it knows which adapter it got.
  lib/           three.js rendering, the thumbnail queue, formatting

src-tauri/src/
  core/          shared by both binaries, depends on neither
    db.rs          SQLite: models, tags, users, sessions
    scanner.rs     streaming walk and hash
    stl.rs         triangle count and bounding box, streaming
    auth.rs        argon2 passwords, session tokens
    thumbs.rs      preview storage, keyed by content hash
  desktop.rs     Tauri commands
  server/main.rs axum server
```

### Why the front end is shell-agnostic

Everything above `src/data/` is written against one small interface. Swapping
Tauri for Electron means writing a main process and using the adapter that is
already there — not rewriting the app. The Pro build needs the HTTP adapter
regardless, so the seam had to exist anyway.

### Why the server is a separate binary

The desktop binary links Tauri, which on Linux pulls in GTK and webkit2gtk. A
NAS is headless and has neither. Cargo features keep them apart:

```
cargo build --bin stlibrarian                                          # desktop
cargo build --no-default-features --features server --bin stlibrarian-server
```

### Previews

Thumbnails are rendered in the front end with three.js and cached as PNGs keyed
by the model's content hash, so a preview can never go stale — a changed file is
a different hash. Rendering is strictly sequential: each one means reading a
whole STL, and there is only one GPU context to share.

## Design language

Nord throughout. Controls are lit slabs — a bevel along the top edge, a hard
edge beneath, a cast shadow — and pressing one collapses that height so it
travels down onto its own shadow. Frost cyan is the only interaction colour;
Aurora colours mean state or category and never decoration. Selection is marked
with corner brackets, the way a targeting UI locks on.
