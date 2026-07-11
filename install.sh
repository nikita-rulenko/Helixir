#!/usr/bin/env bash
# =============================================================================
# Helixir Installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/nikita-rulenko/Helixir/main/install.sh | bash
#
# Or with custom install directory:
#   curl -fsSL ... | bash -s -- --dir ~/my-tools/helixir
# =============================================================================

set -euo pipefail

# Resolved from $INSTALL_DIR/helixir/Cargo.toml after the repo is cloned/updated.
# Until then the banner shows "(detecting)" — see detect_version() below.
# The single source of truth for the version is helixir/Cargo.toml.
VERSION="(detecting)"
REPO="https://github.com/nikita-rulenko/Helixir.git"
DEFAULT_DIR="$HOME/.helixir"
HELIX_PORT=6969

detect_version() {
    local cargo_toml="$1/helixir/Cargo.toml"
    if [ ! -f "$cargo_toml" ]; then
        warn "Cannot read $cargo_toml — keeping VERSION=$VERSION"
        return
    fi
    # Take the first `version = "X.Y.Z"` line in the [package] table.
    # `head -n 1` guards against any nested table also using `version =`.
    VERSION=$(grep -E '^version[[:space:]]*=' "$cargo_toml" \
              | head -n 1 \
              | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        warn "Failed to parse version from $cargo_toml"
        VERSION="unknown"
    fi
}

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info()  { echo -e "${BLUE}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()   { echo -e "${RED}[error]${NC} $*"; }

# ── HelixDB CLI: PINNED to v2.3.5 ────────────────────────────────────────────
# Helixir targets the v2 (LMDB) generation of HelixDB. CLI v3.x is a
# DIFFERENT, INCOMPATIBLE engine (hyperscale over object storage): it has no
# `helix check`/`helix build`, never compiles this repo's schema, and the
# gateway comes up with query_count=0. Both `cargo install helix-cli` and
# `curl install.helix-db.com | bash` install latest (v3) — so this script
# installs the pinned binary itself and HARD-FAILS if a v3 would shadow it.
HELIX_CLI_PIN="2.3.5"
HELIX_CLI_MIRROR="https://github.com/nikita-rulenko/helix-db/releases/download/v${HELIX_CLI_PIN}"

helix_cli_version() { helix --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1; }

ensure_helix_cli() {
    if command -v helix >/dev/null 2>&1; then
        local v; v=$(helix_cli_version)
        case "$v" in
            "$HELIX_CLI_PIN")
                ok "HelixDB CLI v$v (pinned) — $(command -v helix)"; return 0 ;;
            3.*)
                err "Found HelixDB CLI v$v at $(command -v helix) — the v3 generation is INCOMPATIBLE with Helixir."
                err "It cannot compile this repo's schema (no 'helix check'/'helix build'); every Helixir call would fail."
                err "Remove or shadow it, then re-run this script — it installs the pinned v${HELIX_CLI_PIN} automatically:"
                err "    mv \"$(command -v helix)\" \"$(command -v helix).v3\""
                exit 1 ;;
            2.*)
                warn "HelixDB CLI v$v found (pin is v${HELIX_CLI_PIN}). Proceeding — but if 'helix build' misbehaves, install the pin from:"
                warn "    ${HELIX_CLI_MIRROR}"
                return 0 ;;
            *)
                warn "Could not parse 'helix --version' output; replacing with the pinned v${HELIX_CLI_PIN}." ;;
        esac
    fi

    local os arch asset
    os=$(uname -s); arch=$(uname -m)
    case "$os/$arch" in
        Darwin/arm64)          asset="helix-aarch64-apple-darwin" ;;
        Darwin/x86_64)         asset="helix-x86_64-apple-darwin" ;;
        Linux/aarch64|Linux/arm64) asset="helix-aarch64-unknown-linux-gnu" ;;
        Linux/x86_64)          asset="helix-x86_64-unknown-linux-gnu" ;;
        *) err "No pinned HelixDB CLI build for $os/$arch — fetch v${HELIX_CLI_PIN} manually: ${HELIX_CLI_MIRROR}"; exit 1 ;;
    esac

    info "Installing pinned HelixDB CLI v${HELIX_CLI_PIN} ($asset)..."
    mkdir -p "$HOME/.local/bin"
    curl -fsSL --retry 3 -o "$HOME/.local/bin/helix" "${HELIX_CLI_MIRROR}/${asset}" || {
        err "Download failed: ${HELIX_CLI_MIRROR}/${asset}"; exit 1; }
    chmod +x "$HOME/.local/bin/helix"
    export PATH="$HOME/.local/bin:$PATH"

    # the freshly installed pin must be the one PATH resolves
    local got; got=$(helix_cli_version)
    if [ "$got" != "$HELIX_CLI_PIN" ]; then
        err "PATH resolves 'helix' to $(command -v helix) (v${got:-unknown}), not the pinned install."
        err "Put \$HOME/.local/bin FIRST in PATH (and persist it in your shell rc), then re-run."
        exit 1
    fi
    ok "HelixDB CLI v${HELIX_CLI_PIN} installed to ~/.local/bin/helix"
    warn "Add to your shell rc if not present:  export PATH=\"\$HOME/.local/bin:\$PATH\""
}
step()  { echo -e "\n${BOLD}$*${NC}"; }

INSTALL_DIR="$DEFAULT_DIR"
SKIP_DOCKER=false
SKIP_BUILD=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dir)       INSTALL_DIR="$2"; shift 2 ;;
        --skip-docker) SKIP_DOCKER=true; shift ;;
        --skip-build)  SKIP_BUILD=true; shift ;;
        --help|-h)
            echo "Usage: install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --dir PATH       Install directory (default: ~/.helixir)"
            echo "  --skip-docker    Don't start HelixDB container"
            echo "  --skip-build     Don't build from source (use pre-built if available)"
            echo "  --help           Show this help"
            exit 0
            ;;
        *) err "Unknown option: $1"; exit 1 ;;
    esac
done

echo ""
echo -e "${BOLD}  Helixir Installer${NC}"
echo -e "  Graph-based persistent memory for LLM agents"
echo ""

# ── Step 1: Check prerequisites ──────────────────────────────────────────────

step "1/6  Checking prerequisites"

HAS_RUST=false
HAS_DOCKER=false
HAS_GIT=false

if command -v rustc &>/dev/null; then
    RUST_VER=$(rustc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
    ok "Rust $RUST_VER"
    HAS_RUST=true
else
    warn "Rust not found"
fi

if command -v docker &>/dev/null && docker info &>/dev/null 2>&1; then
    ok "Docker"
    HAS_DOCKER=true
else
    warn "Docker not found or not running"
fi

if command -v git &>/dev/null; then
    ok "Git"
    HAS_GIT=true
else
    err "Git is required but not found"
    exit 1
fi

# ── Step 2: Install missing prerequisites ────────────────────────────────────

step "2/6  Installing missing prerequisites"

if [ "$HAS_RUST" = false ]; then
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
    source "$HOME/.cargo/env" 2>/dev/null || true
    ok "Rust installed: $(rustc --version)"
fi

if [ "$HAS_DOCKER" = false ] && [ "$SKIP_DOCKER" = false ]; then
    err "Docker is required for HelixDB but not found."
    echo "  Install Docker: https://docs.docker.com/get-docker/"
    echo "  Or re-run with --skip-docker if HelixDB is running elsewhere."
    exit 1
fi

# ── Step 3: Clone / update repository ────────────────────────────────────────

step "3/6  Setting up $INSTALL_DIR"

if [ -d "$INSTALL_DIR/.git" ]; then
    info "Existing installation found, pulling latest..."
    git -C "$INSTALL_DIR" pull --quiet
    ok "Updated"
elif [ -d "$INSTALL_DIR" ]; then
    warn "$INSTALL_DIR exists but is not a git repo. Using as-is."
else
    info "Cloning repository..."
    git clone --quiet "$REPO" "$INSTALL_DIR"
    ok "Cloned to $INSTALL_DIR"
fi

detect_version "$INSTALL_DIR"
ok "Helixir version: v${VERSION}"

# ── Step 4: Build from source ────────────────────────────────────────────────

step "4/6  Building Helixir"

if [ "$SKIP_BUILD" = true ]; then
    info "Skipping build (--skip-build)"
else
    cd "$INSTALL_DIR/helixir"
    info "Building release binary (this may take 1-2 minutes)..."
    cargo build --release --quiet 2>&1
    ok "Built: helixir-mcp, helixir-deploy"
fi

BINARY_DIR="$INSTALL_DIR/helixir/target/release"
MCP_BIN="$BINARY_DIR/helixir-mcp"
DEPLOY_BIN="$BINARY_DIR/helixir-deploy"

if [ ! -f "$MCP_BIN" ]; then
    err "Binary not found at $MCP_BIN"
    exit 1
fi

# ── Step 5: Start HelixDB + deploy schema ────────────────────────────────────

step "5/6  Setting up HelixDB"

if [ "$SKIP_DOCKER" = true ]; then
    info "Skipping Docker setup (--skip-docker)"
else
    CONTAINER_NAME="helixdb"
    if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        ok "HelixDB container already running"
    else
        if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
            info "Starting existing HelixDB container..."
            docker start "$CONTAINER_NAME" >/dev/null
        else
            # There is NO public helixdb/helixdb image: the server image is
            # built locally by the HelixDB CLI, which compiles our schema
            # INTO it. Build it if missing, then run with the proven flags.
            if ! docker image inspect helix-helixir-dev:latest >/dev/null 2>&1; then
                ensure_helix_cli
                info "Building the HelixDB image from schema (helix build)..."
                (cd "$INSTALL_DIR/helixir/schema" && helix check && helix build -i dev) || {
                    err "helix build failed — see output above"
                    exit 1
                }
            fi
            info "Starting HelixDB container..."
            # -m 3g: OOM containment — a runaway spike restarts the container
            # in seconds instead of OOM-killing the whole Docker VM (LMDB is
            # durable, a restart loses no data). Raise for large corpora.
            # HELIX_DATA_DIR + the volume = disk persistence (newer HelixDB
            # defaults to IN-MEMORY; without this a stop ERASES the data).
            docker run -d \
                --name "$CONTAINER_NAME" \
                -p "${HELIX_PORT}:${HELIX_PORT}" \
                -v helixdb_data:/data \
                -e "HELIX_PORT=${HELIX_PORT}" \
                -e "HELIX_DATA_DIR=/data" \
                --restart unless-stopped \
                -m 3g --memory-swap 3g \
                --log-opt max-size=20m --log-opt max-file=3 \
                helix-helixir-dev:latest >/dev/null
        fi

        info "Waiting for HelixDB to be ready..."
        for i in $(seq 1 30); do
            if curl -sf "http://localhost:${HELIX_PORT}/health" &>/dev/null; then
                break
            fi
            sleep 1
        done
        ok "HelixDB is running on port $HELIX_PORT"
        info "Persistence check: data lives in the 'helixdb_data' volume."
        info "  Newer HelixDB builds default to IN-MEMORY storage — if you ever"
        info "  run an instance outside this script, use disk persistence"
        info "  (helix start dev --disk) or a mounted HELIX_DATA_DIR, and verify"
        info "  a written memory survives a restart before trusting it."
    fi

    info "Deploying schema..."
    "$DEPLOY_BIN" --host localhost --port "$HELIX_PORT" \
        --schema-dir "$INSTALL_DIR/helixir/schema" 2>&1 | tail -2
    ok "Schema deployed"
fi

# ── Step 6: Generate config ──────────────────────────────────────────────────

step "6/6  Configuration"

echo ""
echo -e "${BOLD}Helixir is installed.${NC}"
echo ""
echo "Binary location:"
echo "  $MCP_BIN"
echo ""
echo -e "${BOLD}Next: add Helixir to your IDE.${NC}"
echo ""
echo "For Cursor, add to ~/.cursor/mcp.json:"
echo ""
cat <<JSONEOF
{
  "mcpServers": {
    "helixir": {
      "command": "$MCP_BIN",
      "env": {
        "HELIX_HOST": "localhost",
        "HELIX_PORT": "$HELIX_PORT",
        "HELIX_LLM_PROVIDER": "cerebras",
        "HELIX_LLM_MODEL": "gpt-oss-120b",
        "HELIX_LLM_API_KEY": "YOUR_API_KEY",
        "HELIX_EMBEDDING_PROVIDER": "openai",
        "HELIX_EMBEDDING_MODEL": "nomic-embed-text-v1.5",
        "HELIX_EMBEDDING_URL": "https://openrouter.ai/api/v1",
        "HELIX_EMBEDDING_API_KEY": "YOUR_API_KEY"
      }
    }
  }
}
JSONEOF

echo ""
echo "Get API keys:"
echo "  Cerebras (free):    https://cloud.cerebras.ai"
echo "  OpenRouter (cheap): https://openrouter.ai/keys"
echo "  Or use Ollama for fully local setup (no keys needed)"
echo ""
echo -e "${GREEN}Done.${NC} Restart your IDE to load the MCP server."
echo ""
