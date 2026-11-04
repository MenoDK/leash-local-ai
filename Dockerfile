# syntax=docker/dockerfile:1.7

# ---- build: the server, optimised ----------------------------------------------------------------------------------
# The registry and the target directory are cache mounts, so changing one source file does not download or rebuild the
# world; the binary is copied out inside the same step because a cache mount is not part of the image.
FROM rust:1.99-slim-bookworm AS build
WORKDIR /src
ENV CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock clippy.toml ./
COPY crates ./crates
RUN --mount=type=cache,id=leash-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=leash-cargo-target,target=/src/target \
    cargo build --release --locked -p leash-server \
 && cp target/release/leash-server /leash-server

# ---- runtime: the binary, the site, the demo documents, and nothing that can build or fetch anything --------------
# Debian's base image already carries coreutils, which is all the demo commands need (echo, uname, date). There is no
# shell script, no package manager call, no curl: the health check is the server asking itself.
FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin leash \
 && mkdir /data \
 && chown 10001:10001 /data
COPY --from=build /leash-server /usr/local/bin/leash-server
COPY site /app/site
COPY corpus /app/corpus
ENV LEASH_ADDR=0.0.0.0:8090 \
    LEASH_DATA_DIR=/data \
    LEASH_SITE_DIR=/app/site \
    LEASH_CORPUS_DIR=/app/corpus
WORKDIR /app
USER 10001:10001
EXPOSE 8090
VOLUME ["/data"]
HEALTHCHECK --interval=5s --timeout=4s --start-period=5s --retries=12 \
  CMD ["/usr/local/bin/leash-server", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/leash-server"]
