# stlibrarian.com

One static page. No build step, no dependencies to install -- upload
`index.html` and it is the site.

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
