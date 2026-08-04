# ============================================================================
# Helixir-RS Dockerfile
# Multi-stage build for minimal production image
# ============================================================================

# Stage 1: Build
FROM rust:1.88-slim-bookworm AS builder

ARG TARGETARCH

# Install build dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# NLI is mandatory. Pin Microsoft's runtime per target and verify the archive
# before Cargo links against it; the shared library is copied into the runtime
# image below.
RUN case "$TARGETARCH" in \
      amd64) \
        ort_asset="onnxruntime-linux-x64-1.24.2"; \
        ort_sha="43725474ba5663642e17684717946693850e2005efbd724ac72da278fead25e6" ;; \
      arm64) \
        ort_asset="onnxruntime-linux-aarch64-1.24.2"; \
        ort_sha="6715b3d19965a2a6981e78ed4ba24f17a8c30d2d26420dbed10aac7ceca0085e" ;; \
      *) echo "unsupported Docker architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac && \
    curl -fsSL --retry 5 --retry-delay 5 \
      -o /tmp/onnxruntime.tgz \
      "https://github.com/microsoft/onnxruntime/releases/download/v1.24.2/${ort_asset}.tgz" && \
    echo "${ort_sha}  /tmp/onnxruntime.tgz" | sha256sum -c - && \
    mkdir -p /opt/onnxruntime && \
    tar xzf /tmp/onnxruntime.tgz --strip-components=1 -C /opt/onnxruntime && \
    rm /tmp/onnxruntime.tgz

ENV ORT_LIB_LOCATION=/opt/onnxruntime/lib
ENV ORT_PREFER_DYNAMIC_LINK=1

WORKDIR /app

# Copy manifests first (for layer caching)
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/bin/helixir.rs && \
    echo "fn main() {}" > src/bin/helixir_bench.rs && \
    echo "fn main() {}" > src/bin/helixir_mcp.rs && \
    echo "fn main() {}" > src/bin/helixir_deploy.rs && \
    echo "pub fn dummy() {}" > src/lib.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src
COPY memory-charter.md ./memory-charter.md

# Build the real binaries
RUN touch src/lib.rs src/bin/helixir_mcp.rs src/bin/helixir_deploy.rs && \
    cargo build --release --bin helixir --bin helixir-mcp --bin helixir-deploy

# The runtime image must be ready without a hidden post-build model step.
RUN LD_LIBRARY_PATH=/opt/onnxruntime/lib /app/target/release/helixir model download

# ============================================================================
# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /opt/onnxruntime/lib/libonnxruntime.so* /usr/local/lib/
RUN ldconfig

# Create non-root user
RUN useradd -m -u 1000 helixir
USER helixir
WORKDIR /home/helixir

# Copy binary from builder
COPY --from=builder /app/target/release/helixir-mcp /usr/local/bin/helixir-mcp
COPY --from=builder /app/target/release/helixir-deploy /usr/local/bin/helixir-deploy
COPY --from=builder /app/target/release/helixir /usr/local/bin/helixir
COPY --from=builder --chown=helixir:helixir /root/.helixir/models/nli/ /home/helixir/.helixir/models/nli/

# Copy schema files (for deployment)
COPY --chown=helixir:helixir schema/ ./schema/

# Environment variables (defaults)
ENV HELIX_HOST=localhost
ENV HELIX_PORT=6969
ENV HELIX_INSTANCE=default
ENV HELIX_LLM_PROVIDER=cerebras
ENV HELIX_LLM_MODEL=gpt-oss-120b
ENV HELIX_EMBEDDING_PROVIDER=openai
ENV HELIX_EMBEDDING_MODEL=sentence-transformers/all-mpnet-base-v2
ENV RUST_LOG=helixir=warn,helixir::mcp=info

# Expose nothing - MCP uses stdio
# EXPOSE 8080

# Health check (optional, for orchestrators)
# HEALTHCHECK --interval=30s --timeout=3s \
#     CMD echo '{"jsonrpc":"2.0","method":"ping","id":1}' | helixir-mcp || exit 1

# Default command
ENTRYPOINT ["helixir-mcp"]
