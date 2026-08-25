FROM lukemathwalker/cargo-chef:latest-rust-1@sha256:d21902576546863995017609e991178939e6f151f6b35fb00f3c0fa97a616ef5 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin helix-db

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/helix-db /usr/local/bin
ENTRYPOINT ["/usr/local/bin/helix-db"]
