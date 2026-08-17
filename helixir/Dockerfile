# ============================================================================
# Helixir-RS Dockerfile
# Multi-stage build for minimal production image
# ============================================================================

# Stage 1: Compile the browser application. These assets exist only in the
# control-plane image; native installation never copies them onto the host.
FROM node:22-bookworm-slim AS web-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/index.html web/tsconfig.json web/tsconfig.app.json web/vite.config.ts ./
COPY web/src ./src
RUN npm run build

# Stage 2: Build Rust binaries
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

# The native MCP runtime must be ready without a hidden post-build model step.
# Keep model acquisition outside the shared binary builder so the web-only
# image does not download or carry NLI weights it never executes.
FROM builder AS model-builder
RUN LD_LIBRARY_PATH=/opt/onnxruntime/lib /app/target/release/helixir model download

# ============================================================================
# Stage 3: Shared native runtime
FROM debian:bookworm-slim AS runtime-base

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /opt/onnxruntime/lib/libonnxruntime.so* /usr/local/lib/
RUN ldconfig

# Create non-root user
RUN useradd -m -u 1000 helixir
USER helixir
WORKDIR /home/helixir

# Environment variables (defaults)
ENV HELIX_HOST=localhost
ENV HELIX_PORT=6969
ENV HELIX_INSTANCE=default
ENV HELIX_LLM_PROVIDER=cerebras
ENV HELIX_LLM_MODEL=gpt-oss-120b
ENV HELIX_EMBEDDING_PROVIDER=ollama
ENV HELIX_EMBEDDING_MODEL=nomic-embed-text
ENV HELIX_EMBEDDING_URL=http://localhost:11434
ENV RUST_LOG=helixir=warn,helixir::mcp=info

# Stage 4a: Native MCP runtime. It remains a stdio process and contains no
# browser assets.
FROM runtime-base AS runtime

COPY --from=builder /app/target/release/helixir-mcp /usr/local/bin/helixir-mcp
COPY --from=builder /app/target/release/helixir-deploy /usr/local/bin/helixir-deploy
COPY --from=builder /app/target/release/helixir /usr/local/bin/helixir
COPY --from=model-builder --chown=helixir:helixir /root/.helixir/models/nli/ /home/helixir/.helixir/models/nli/
COPY --chown=helixir:helixir schema/ ./schema/

ENTRYPOINT ["helixir-mcp"]

# Stage 4b: Permanent admin-only web control plane. Docker isolation is the
# packaging boundary: no Docker socket or host home directories are needed.
FROM runtime-base AS control-plane

COPY --from=builder /app/target/release/helixir /usr/local/bin/helixir
COPY --from=web-builder --chown=helixir:helixir /web/dist/ /opt/helixir/web/

ENV HELIXIR_WEB_DIST=/opt/helixir/web
ENV HELIX_HOST=helixdb
ENV HELIX_PORT=6969

EXPOSE 6971

HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD curl -fsS http://127.0.0.1:6971/ >/dev/null || exit 1

ENTRYPOINT ["helixir", "web", "--bind", "0.0.0.0:6971", "--no-open", "--containerized"]
