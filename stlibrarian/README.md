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

Two things that will bite if you change them. The npm `@tauri-apps/plugin-*`
packages and their Rust crates are pinned to the same minor on purpose -- the
Tauri CLI refuses to start if they disagree, so bump both together or neither.
And `default-run` in `Cargo.toml` is what lets `tauri dev` work at all, because
`tauri dev` shells out to a bare `cargo run` and this package has two binaries.

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

## What it knows about a model

Scanning reads `.stl`, `.obj` and `.3mf` and records size, triangle count,
bounding box and enclosed volume. Volume matters more than it sounds: multiply
by a resin density and you have the grams a print will cost, which is the
number you want before opening a new bottle. Bounding box answers "will this
fit on my bed?" without opening anything.

Volume is only meaningful for a closed mesh, so each file is also checked for
watertightness -- every edge shared by exactly two triangles. That is computed
in constant memory by XOR-ing a hash of each edge, because holding nine million
edges for a three million triangle model would cost hundreds of megabytes to
learn one boolean. A mesh with holes is flagged, its volume is not dressed up
as a resin weight, and it is the kind of file that may not slice cleanly.

Each file also gets a **variant** -- supported or unsupported -- read from its
path, because packs ship both and printing the wrong one wastes an evening.
And a **creator** and **source**, guessed from the folder layout and freely
editable. A rescan never overwrites a correction.

## Previews

Rendered in Rust. A software rasteriser streams the mesh a triangle at a time,
so a three million triangle model costs the same memory as a cube and comes
out at full detail -- the thumbnail shows more than the interactive viewer
does. Nothing needs a GPU, a window, or a browser, which is what lets a
headless NAS build its own previews instead of waiting for somebody to visit
it.

When a creator ships their own render beside the mesh, that is used instead.
It is posed, lit and often painted; nothing generated from raw geometry
competes. The inspector lets you override the choice or go back to the
generated one.

The interactive viewer draws Solid, Wire or Points. Points has no surface to
hide behind, so interior structure and wall thickness show through.

## Organising a library

**Tags** describe a property the members share. Type one in the inspector, or
apply one to a whole selection at once. Filtering by several tags narrows
(AND), so `dragons` + `printed` means both, not either.

**Collections** are named sets you build by hand — Dragons, Needs Supports,
Christmas Gifts. A model can sit in any number of them. They are a different
idea from tags on purpose: a tag is something the members have in common, a
collection is simply what you put in it.

Selection follows the conventions every file manager uses:

| Gesture | Effect |
| --- | --- |
| click | open one model in the inspector |
| cmd/ctrl-click | add or remove one model from the selection |
| shift-click | extend the selection from the last anchor |
| arrow keys | move the inspector focus through the grid |
| escape | clear the selection |

With anything selected, a bar appears at the bottom of the grid: add or remove
a tag across the whole selection, add it to a collection, or group it into a
package. It reports what actually changed, so "tagged 12" of a selection of 40
tells you the other 28 already had it.

**Packages** are several files that are one model -- a body, two wings and an
arm. Scanning *suggests* groupings and never applies them, because a folder of
STLs is as often a folder of unrelated models as it is one model in pieces.
Suggestions collapse variant folders, so the supported and unsupported copies
of a model are recognised as the same model.

**Duplicates** are found from the content hashes every file already carries. It
reports what is stored twice and what keeping one copy would return; it does
not delete anything.

**Prints** record what you actually made: printer, material, settings, outcome,
notes. Failures count too -- how many attempts a model took is the interesting
part. The grid marks what you have printed, and the top bar filters to what you
never have, which is the question a pile of downloads cannot answer.

## Tests

```bash
cd src-tauri && cargo test --no-default-features --lib   # core: 101 tests
tools/e2e.sh                                             # server + API: 114 checks
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
    db.rs          SQLite: models, tags, collections, packages, prints, users
    raster.rs      software renderer -- previews with no GPU
    organize.rs    the only code that writes to your files (Pro)
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

### Orbiting a model

Left-drag orbits, scroll zooms, right-drag pans, and Reset re-frames if you
lose the model off screen. Auto-spin rotates the mesh rather than the camera,
so it does not fight an orbit you set yourself.

## Pro: organising files on disk

Everything above is metadata. Scanning reads and never writes, so the desktop
client is a front end over a database and your folders are left exactly as they
are.

Rearranging files for real is the Pro server's job. It files models as
`creator / model / variant`, plans every move before making one, never
overwrites anything, and recomputes the plan itself rather than trusting one
sent by a client. It exists once, on the server, rather than once per desktop
platform -- which is the other reason it belongs there.

## Design language

Nord throughout. Controls are lit slabs — a bevel along the top edge, a hard
edge beneath, a cast shadow — and pressing one collapses that height so it
travels down onto its own shadow. Frost cyan is the only interaction colour;
Aurora colours mean state or category and never decoration. Selection is marked
with corner brackets, the way a targeting UI locks on.
