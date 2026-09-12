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
#
# The two build stages are pinned to $BUILDPLATFORM and cross-compile, rather
# than being emulated per target. Measured, on the release runner: emulated
# arm64 spent 83m42s on the same cargo build that takes 7m12s native, an 11.6x
# tax on every push. Cross-compiling costs one apt package instead.

# --------------------------------------------------------------- web build ---
# Pinned to the build host: the output is JavaScript and CSS, identical
# whatever it will eventually run on. Unpinned it would be built once per
# target architecture, the second time under emulation, for the same bytes.
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS web
WORKDIR /src
# Manifests first: dependencies change far less often than the UI, so this
# layer survives most rebuilds.
COPY package.json yarn.lock ./
RUN yarn install --frozen-lockfile
COPY index.html vite.config.js ./
COPY src ./src
RUN yarn build

# ------------------------------------------------------------ server build ---
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS server
ARG TARGETARCH
WORKDIR /src

# Resolve the target triple and its C compiler once, and write them where the
# build step can read them. rusqlite is built with `bundled`, so SQLite is
# compiled from source and a C cross compiler is genuinely needed, not just a
# Rust one -- and that is also why the runtime image needs no libsqlite3 and
# can be a slim base rather than whatever the host happens to ship.
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-gnu;  cc=x86_64-linux-gnu-gcc; \
             pkg="gcc-x86-64-linux-gnu libc6-dev-amd64-cross"  ;; \
      arm64) triple=aarch64-unknown-linux-gnu; cc=aarch64-linux-gnu-gcc; \
             pkg="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /triple; \
    rustup target add "$triple"; \
    # Native for one of the two platforms, so do not pay for a cross toolchain
    # that will not be used.
    if [ "$(dpkg --print-architecture)" = "$TARGETARCH" ]; then \
      echo cc > /cc; \
    else \
      echo "$cc" > /cc; \
      apt-get update; \
      # The libc6-dev-*-cross package is only a Recommends of the compiler, so
      # --no-install-recommends drops it and the cross gcc silently falls back
      # to the host's /usr/include -- which fails deep inside SQLite's build
      # with "bits/libc-header-start.h: No such file or directory". Named
      # explicitly so that cannot happen again.
      apt-get install -y --no-install-recommends $pkg; \
      rm -rf /var/lib/apt/lists/*; \
    fi

COPY src-tauri ./src-tauri
# The web UI is compiled into the binary, so it has to be here before cargo
# runs -- not copied into the runtime image afterwards.
COPY --from=web /src/dist ./dist

# The target cache is keyed by architecture. Both platforms build at once on
# the same host now, and a shared target directory would have them overwriting
# each other's artifacts.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=stl-cargo-registry,sharing=locked \
    --mount=type=cache,target=/src/src-tauri/target,id=stl-cargo-target-$TARGETARCH \
    set -eux; \
    triple="$(cat /triple)"; cc="$(cat /cc)"; \
    # cargo reads the linker from CARGO_TARGET_<TRIPLE>_LINKER and the cc crate
    # reads the compiler from CC_<triple>, spelled differently: upper with
    # dashes as underscores for the first, lower with underscores for the
    # second. Both are needed -- the first links the Rust code, the second
    # compiles SQLite.
    upper="$(echo "$triple" | tr 'a-z-' 'A-Z_')"; \
    lower="$(echo "$triple" | tr '-' '_')"; \
    export "CARGO_TARGET_${upper}_LINKER=$cc"; \
    export "CC_${lower}=$cc"; \
    cargo build --release --locked --target "$triple" \
        --manifest-path src-tauri/Cargo.toml \
        --no-default-features --features server --bin stlibrarian-server; \
    # Out of the cache mount before it is unmounted, or the next stage finds
    # nothing to copy.
    cp "src-tauri/target/$triple/release/stlibrarian-server" \
       /usr/local/bin/stlibrarian-server

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
