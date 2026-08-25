FROM rust:1.88-bookworm AS builder
WORKDIR /workspace
COPY --from=helixir-schema queries.hx helixir/schema/queries.hx
COPY . helixdb-mock
RUN cargo build --manifest-path helixdb-mock/Cargo.toml --release --locked

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 helixdb-mock
COPY --from=builder /workspace/helixdb-mock/target/release/helixdb-mock /usr/local/bin/helixdb-mock
USER helixdb-mock
EXPOSE 16969
ENV HELIXDB_MOCK_LISTEN=0.0.0.0:16969
ENTRYPOINT ["/usr/local/bin/helixdb-mock"]
