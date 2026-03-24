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

VERSION="0.2.2"
REPO="https://github.com/nikita-rulenko/Helixir.git"
DEFAULT_DIR="$HOME/.helixir"
HELIX_PORT=6969

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
echo -e "${BOLD}  Helixir Installer v${VERSION}${NC}"
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
            info "Starting HelixDB container..."
            docker run -d \
                --name "$CONTAINER_NAME" \
                -p "${HELIX_PORT}:${HELIX_PORT}" \
                -v helixdb_data:/data \
                --restart unless-stopped \
                helixdb/helixdb:latest >/dev/null
        fi

        info "Waiting for HelixDB to be ready..."
        for i in $(seq 1 30); do
            if curl -sf "http://localhost:${HELIX_PORT}/health" &>/dev/null; then
                break
            fi
            sleep 1
        done
        ok "HelixDB is running on port $HELIX_PORT"
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
