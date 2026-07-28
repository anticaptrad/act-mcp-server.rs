# Multi-stage build for the act-mcp-server Rust binary.
FROM rust:1-bookworm AS builder
WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --no-create-home appuser
USER 10001

WORKDIR /app
COPY --from=builder /usr/src/app/target/release/act_mcp_server /usr/local/bin/act-mcp-server
COPY .cli-flags.toml /app/.cli-flags.toml

ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["act-mcp-server"]
