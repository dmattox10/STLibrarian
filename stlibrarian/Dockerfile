# syntax=docker/dockerfile:1
#
# STLibrarian Pro, as one container.
#
# The desktop app ships as a bundle you double-click. The server had no
# equivalent -- installing it meant a Rust toolchain, a Node toolchain, two
# builds and an environment variable pointing at the right directory, on a NAS
# that has none of those things. This image is that missing bundle: everything
# compiled here, nothing needed on the target but Docker.
#
#   docker compose up -d
#
# Build for a NAS that is not this machine's architecture with buildx:
#   docker buildx build --platform linux/amd64,linux/arm64 -t stlibrarian:latest .

# --------------------------------------------------------------- web build ---
FROM node:22-bookworm-slim AS web
WORKDIR /src
# Manifests first: dependencies change far less often than the UI, so this
# layer survives most rebuilds.
COPY package.json yarn.lock ./
RUN yarn install --frozen-lockfile
COPY index.html vite.config.js ./
COPY src ./src
RUN yarn build

# ------------------------------------------------------------ server build ---
FROM rust:1-bookworm AS server
WORKDIR /src
# rusqlite is built with `bundled`, so SQLite is compiled from source here and
# the runtime image needs no libsqlite3 at all. That is the whole reason this
# image can be a slim base rather than whatever the host happens to ship.
COPY src-tauri ./src-tauri
# The web UI is compiled into the binary, so it has to be here before cargo
# runs -- not copied into the runtime image afterwards.
COPY --from=web /src/dist ./dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/src-tauri/target \
    cargo build --release --locked --manifest-path src-tauri/Cargo.toml \
        --no-default-features --features server --bin stlibrarian-server \
 && cp src-tauri/target/release/stlibrarian-server /usr/local/bin/stlibrarian-server

# ---------------------------------------------------------------- runtime ----
FROM debian:bookworm-slim AS runtime

# curl is here for the healthcheck and nothing else. A container that cannot
# say whether it is ready is a container a NAS UI shows as green while the
# library is unreachable.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# One file. The web UI is inside it.
COPY --from=server /usr/local/bin/stlibrarian-server /usr/local/bin/stlibrarian-server

# The library database and rendered thumbnails. Mount a volume here or the
# library is lost with the container.
ENV STLIBRARIAN_DATA_DIR=/data \
    STLIBRARIAN_BIND=0.0.0.0:3000
VOLUME ["/data"]

# Models live outside the image, mounted from the NAS share. Declared so that
# `docker run` without a mount fails visibly on an empty folder rather than
# writing a library into the container's own filesystem.
RUN mkdir -p /data /library

EXPOSE 3000

# Runs as root by default because a NAS share's files belong to whoever the NAS
# says, and guessing wrong means a library that cannot be read. Set
# `user: "1000:1000"` in compose to match your share, once you know it.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:3000/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/stlibrarian-server"]
