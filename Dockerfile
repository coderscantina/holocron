# One-process Holocron image: the release binary with the built dashboard beside it.
#
#   docker build --build-arg HOLOCRON_GIT_SHA=$(git rev-parse --short HEAD) -t holocron .
#
# The data directory is /data. On an empty volume `serve` starts in setup mode and logs a setup
# token for the wizard in the browser; `holocron init` or `holocron restore` still work instead.
# See docs/guide/install.md.
#
# Dependencies, DuckDB above all, compile in a layer of their own that only Cargo.toml and
# Cargo.lock invalidate (cargo-chef). With a layer cache, a release that changes no dependency
# compiles only Holocron's own crates.

FROM oven/bun:1.4 AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web ./
RUN bun run build

FROM rust:1.99-trixie AS chef
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY apps/server apps/server
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server
# Lower it (--build-arg CARGO_BUILD_JOBS=2) on builders with little memory: DuckDB's C++
# compile needs a lot of memory per job.
ARG CARGO_BUILD_JOBS
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked -p holocron-server --bin holocron --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY apps/server apps/server
COPY crates crates
# The tracker and recorder scripts and the import schemas are compiled into the binary.
COPY sources sources
COPY fixtures/import-schemas fixtures/import-schemas
# The release commit, shown by `holocron --version`. Declared late so it invalidates no
# layer above.
ARG HOLOCRON_GIT_SHA
RUN cargo build --release --locked -p holocron-server --bin holocron \
    && cp target/release/holocron /holocron

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data holocron \
    && install -d -o holocron -g holocron /data
# `serve` finds the dashboard in the `web` directory next to the executable.
COPY --from=server /holocron /opt/holocron/holocron
COPY --from=web /src/apps/web/dist /opt/holocron/web
ENV PATH=/opt/holocron:$PATH \
    HOLOCRON_DATA_DIR=/data \
    HOLOCRON_BIND=0.0.0.0:8080
USER holocron
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["holocron"]
CMD ["serve"]
