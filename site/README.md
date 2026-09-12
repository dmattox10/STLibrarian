# stlibrarian.com

One static page and three screenshots. No build step, no dependencies to
install -- upload `index.html` and `shots/` and it is the site.

## Screenshots

`shots/` holds the three JPEGs in the "What it looks like" section. They are
photographs of the real application: the headless server running against a
throwaway library, driven in a real browser. Nothing is a mockup and no DOM is
edited to make a shot look better.

Regenerate them with:

```bash
site/tools/screenshots.sh
```

That builds the front end and the server, generates a sample library of real
watertight meshes, scans and seeds it through the HTTP API, photographs three
views at 2x, and downsamples them into `shots/`. Review what lands there before
committing -- it is a screenshot, and only a person can tell whether it reads.

Three things about it are deliberate and worth not undoing:

- **The library lives at `/volume1/3dprints`.** The path is on screen in the
  details panel. Pointing the rig at a build directory puts that build
  directory on the marketing site.
- **No reviews are seeded.** The page renders `/api/reviews`, and the point of
  that feature is that a review comes from somebody who runs the software and
  is approved by a person first. Inventing testimonials to photograph would
  defeat the one thing it exists to prevent.
- **Provenance is left as the scanner inferred it.** Creator comes from the
  folder name. Overwriting it with invented designer names would make generated
  sample geometry look like somebody else's work.

The shots are of the server in a browser, which is why a sign-out button is
visible. The page says so, because the desktop app is the same front end built
from the same source and the difference is worth being straight about.

## Reviews

The page fetches `/api/reviews` on load and renders whatever comes back. If the
request fails, or the list is empty, the section stays hidden rather than
showing an apology for content nobody promised -- so the page is correct from
the day it goes up, before a single review exists.

That means the site wants to be served from the same origin as an STLibrarian
server, or behind a proxy that forwards `/api/reviews` to one. Served as a
plain static file with nothing behind it, the page works perfectly and simply
has no reviews section.

**There is no submission form here on purpose.** The app does the asking, so a
review comes from somebody who actually runs the software, and every one is
approved by a person before it can appear. See "Reviews and problem reports"
in the main README.

## Downloads

The three buttons point at `github.com/dmattox10/stlibrarian-releases`, which
is the public releases repository -- the source repo is private and its assets
cannot be fetched without a token. See "Releasing" in the main README.

## A note on encoding

The file is deliberately pure ASCII: dashes and stars are written as HTML
entities and `\u` escapes. It carries no `<head>` of its own, so it has nowhere
to declare a charset, and a host that serves it without one would otherwise
turn every em dash into mojibake. Keep it that way.
