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

## Where your library lives

Everything you tell STLibrarian -- tags, packages, collections, print history,
provenance you corrected by hand -- is in one SQLite file. Your models stay
wherever you keep them and are never copied into it.

| | |
| --- | --- |
| macOS | `~/Library/Application Support/STLibrarian/` |
| Linux | `~/.local/share/STLibrarian/` |
| Windows | `%APPDATA%\STLibrarian\` |
| Anywhere | `STLIBRARIAN_DATA_DIR` overrides all of the above |

That folder holds `stl.db` and a `thumbnails/` directory. **Back up `stl.db`
and you have kept all of it.** Previews are drawn from the models and rebuild
themselves, so a backup that skips them loses nothing but time.

Close the app first, or copy `stl.db-wal` and `stl.db-shm` alongside it --
those hold writes that have not been folded into the main file yet, and a copy
without them can be a moment stale.

The app shows you this path, with a button to reveal it, under **Backup** in
the sidebar.

## When files move

Every row stores an absolute path, which is fine until it is not: a library
moved to a bigger disk, a share remounted somewhere else, or a database
restored onto a machine that keeps its models somewhere different. Then every
path is wrong at once and the library looks empty even though nothing is lost.

Nothing is ever deleted for being missing -- an unplugged drive must not empty
somebody's library. Instead **Missing files** appears in the sidebar. Point it
at where the models are now and it matches them **by content**, so renames and
reorganised folders make no difference. Only files whose size already matches
something missing are opened, so it does not re-read the whole library to find
a few strays.

For what searching cannot reach -- a model re-exported at a different
resolution, a mesh someone repaired -- **Locate** points one model at one file
by hand. The row is then re-measured from that file rather than assuming it is
the old one, so its triangle count and volume describe what is actually there.

## Tests

```bash
cd src-tauri && cargo test --features server --lib        # everything: 155 tests
cd src-tauri && cargo test --no-default-features --lib    # core only, no Tauri
yarn test                                                 # the client's own gates
tools/e2e.sh                                              # server + API: 204 checks
```

`tools/e2e.sh` builds everything, starts a server on a scratch port against a
scratch data directory, and exercises the API the way a client does. It touches
nothing you own.

## Reviews and problem reports

The app asks for a review once, and only after the library has earned the
question: twenty-five models with three prints logged against them. A fresh
install is never asked, and a big import with nothing printed is a folder, not
a habit. "No thanks" ends it permanently. **Feedback** in the sidebar is always
there for anyone who wants to say something unprompted.

Submissions arrive at an open endpoint -- a copy of the app on a bench
somewhere, with no account anywhere -- so they are treated as hostile. Control
characters are stripped, paragraphs kept, lengths capped, and an address may
send six an hour (`STLIBRARIAN_FEEDBACK_PER_HOUR` raises that, which you want
behind a reverse proxy, where every submission arrives from one address).

Nothing submitted is public. A review reaches `GET /api/reviews` only if **it
is a review**, **an administrator approved it**, and **its author ticked the
publish box**. Approving one the author withheld files it as read. A problem
report is never a testimonial however warmly it is worded, and the public shape
carries no contact details and no status at all. The moderation queue is under
**Feedback** in a Pro sidebar, administrators only.

The website reads `/api/reviews` at load and stays hidden when it is empty.
There is no submission form on the site on purpose -- the app does the asking,
so a review comes from somebody who actually runs it.

## Releasing

This repository is private, and release assets on a private repo need a token
to download, which a download button on a website cannot supply. So the source
stays here and only the built artifacts cross over to a **separate public
repository** that holds releases and nothing else.

`tools/setup-releases.sh` checks every piece and tells you what is missing.
Run it any time; it changes nothing unless you ask it to.

The public repository, `dmattox10/stlibrarian-releases`, already exists and
holds builds only. What remains is a token, because GitHub has no API that
mints one: make a **fine-grained** token at
<https://github.com/settings/personal-access-tokens/new>, scoped to *that
repository only* with **Contents: read and write**, then

```bash
gh secret set RELEASES_TOKEN --repo dmattox10/STLibrarian
```

and paste it. Scoped that way it cannot read this repository, which is the
point of using one rather than a token that reaches everything you own.

To check the token really works before trusting it to a midnight release:

```bash
RELEASES_TOKEN=github_pat_... tools/setup-releases.sh
```

That cuts a throwaway draft release in the public repo and deletes it again. A
draft is visible to nobody and notifies nobody.

Then a release is a tag:

```bash
# bump the version in package.json, src-tauri/Cargo.toml and tauri.conf.json
git tag v0.2.0 && git push origin v0.2.0
```

One caveat while this work is on a branch: `workflow_dispatch` only registers
a workflow that exists on the default branch, so the manual run is unavailable
until it lands on `main`. Tag-triggered runs use the workflow as it exists at
the tagged commit, so tagging a branch works today.

`.github/workflows/release.yml` runs the test suites on every platform, builds
the desktop bundles for macOS (both architectures), Windows and Linux, builds
the headless server for x86_64 and arm64, and publishes the lot. A manual run
(**Run workflow**) builds everything and publishes nothing, so the pipeline can
be exercised without putting anything in front of anybody.

The server job deliberately installs no GUI packages. If it ever needs them,
something has leaked from the desktop half into the shared core.

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

## Sliced files

A model is not printable. The gap between an STL and a print is slicing, and
slicing is supports, orientation and profiles -- an enormous problem that
belongs to PrusaSlicer and Lychee, not here.

What a library can do is keep the result. Save a slicer's output beside the
model and the next scan picks it up, reads the slicer's own comments for layer
height, print time, material and printer, and attaches it. A print record can
then point at the exact file that was run, so repeating a job is a click rather
than a memory test.

Attachment happens only when it is unambiguous -- a slice named after a model,
or alone beside one. A folder holding six models and a plate file is left for
you to place, because guessing there attaches the wrong output to the wrong
model. Plates sliced from a package attach to the package, so every part can
find them.

Only text gcode is parsed. The resin formats are vendor binaries with versioned
and sometimes obfuscated headers; guessing at those would produce confident
wrong numbers, which is worse than none, so a `.ctb` shows its size and nothing
it has not earned.

## Pointing the desktop app at a server

The desktop client can read a remote library instead of its own disk. There is
exactly one reason to do that rather than opening the web UI: a browser can
offer you a download, but it cannot put the file into your slicer. Connected to
a server, the desktop client still can -- it fetches the file in Rust, caches
it, and hands it to whatever opens that format here.

That is the shape a workshop actually has: the library on a NAS, the printers
and slicers at workstations.

## Pro: people

Two roles. An administrator scans, organises, deletes models and manages
people; everyone else browses, tags, collects and logs prints. More roles than
that is a permissions matrix nobody maintains.

The last administrator cannot step down or be removed -- a server where nobody
can promote anyone is only fixable by editing the database by hand.

Prints record who ran them, and removing someone keeps their history: a shop's
account of what came off which printer outlives anybody's account. Changing a
password ends that person's existing sessions, because a password change
usually means the old one leaked.

Tags and packages are shared -- they are the library's own structure. A
collection can be either: shared by default, or just for you, which is the
difference between the shop's shelves and somebody's print queue.

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
