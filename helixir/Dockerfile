# ============================================================================
# Helixir-RS Dockerfile
# Multi-stage build for minimal production image
# ============================================================================

# Stage 1: Build
FROM rust:1.83-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first (for layer caching)
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/bin/helixir_mcp.rs && \
    echo "pub fn dummy() {}" > src/lib.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src

# Build the real binary
RUN touch src/lib.rs src/bin/helixir_mcp.rs && \
    cargo build --release --bin helixir-mcp

# ============================================================================
# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 helixir
USER helixir
WORKDIR /home/helixir

# Copy binary from builder
COPY --from=builder /app/target/release/helixir-mcp /usr/local/bin/helixir-mcp

# Copy schema files (for deployment)
COPY --chown=helixir:helixir schema/ ./schema/

# Environment variables (defaults)
ENV HELIX_HOST=localhost
ENV HELIX_PORT=6969
ENV HELIX_INSTANCE=default
ENV LLM_PROVIDER=cerebras
ENV LLM_MODEL=llama-3.3-70b
ENV EMBEDDING_PROVIDER=openai
ENV EMBEDDING_MODEL=sentence-transformers/all-mpnet-base-v2
ENV RUST_LOG=helixir=warn,helixir::mcp=info

# Expose nothing - MCP uses stdio
# EXPOSE 8080

# Health check (optional, for orchestrators)
# HEALTHCHECK --interval=30s --timeout=3s \
#     CMD echo '{"jsonrpc":"2.0","method":"ping","id":1}' | helixir-mcp || exit 1

# Default command
ENTRYPOINT ["helixir-mcp"]

