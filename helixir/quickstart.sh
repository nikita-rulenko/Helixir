#!/bin/bash
# ============================================================================
# Helixir Quickstart Script
# 
# One-command setup for Helixir memory framework
# ============================================================================

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}"
echo "  _   _      _ _      _      "
echo " | | | | ___| (_)_  _(_)_ __ "
echo " | |_| |/ _ \ | \ \/ / | '__|"
echo " |  _  |  __/ | |>  <| | |   "
echo " |_| |_|\___|_|_/_/\_\_|_|   "
echo -e "${NC}"
echo "🧠 Ontological Memory for LLM Agents"
echo ""

# Check Docker
check_docker() {
    if ! command -v docker &> /dev/null; then
        echo -e "${RED}❌ Docker not found${NC}"
        echo ""
        echo "Please install Docker first:"
        echo "  macOS:   brew install --cask docker"
        echo "  Linux:   curl -fsSL https://get.docker.com | sh"
        echo "  Windows: https://docs.docker.com/desktop/install/windows-install/"
        exit 1
    fi
    
    if ! docker info &> /dev/null; then
        echo -e "${RED}❌ Docker daemon not running${NC}"
        echo "Please start Docker Desktop or docker service"
        exit 1
    fi
    
    echo -e "${GREEN}✓ Docker found${NC}"
}

# Check Docker Compose
check_compose() {
    if docker compose version &> /dev/null; then
        COMPOSE_CMD="docker compose"
    elif command -v docker-compose &> /dev/null; then
        COMPOSE_CMD="docker-compose"
    else
        echo -e "${RED}❌ Docker Compose not found${NC}"
        exit 1
    fi
    echo -e "${GREEN}✓ Docker Compose found${NC}"
}

# Create .env file
create_env() {
    if [ -f .env ]; then
        echo -e "${YELLOW}⚠ .env file exists, skipping${NC}"
        return
    fi
    
    echo ""
    echo -e "${BLUE}📝 Configure API Keys${NC}"
    echo ""
    echo "You need API keys for LLM and embeddings."
    echo "Options:"
    echo "  1. Cerebras (free tier): https://cloud.cerebras.ai"
    echo "  2. OpenRouter (cheap):   https://openrouter.ai/keys"
    echo "  3. OpenAI:               https://platform.openai.com/api-keys"
    echo "  4. Ollama (local):       No keys needed"
    echo ""
    
    read -p "Use Ollama for fully local setup? [y/N] " use_ollama
    
    if [[ "$use_ollama" =~ ^[Yy]$ ]]; then
        cat > .env << 'EOF'
# Helixir Configuration - Local (Ollama)
# Make sure Ollama is running: ollama serve
# Pull models: ollama pull llama3:8b && ollama pull nomic-embed-text

LLM_PROVIDER=ollama
LLM_MODEL=llama3:8b
LLM_BASE_URL=http://host.docker.internal:11434

EMBEDDING_PROVIDER=ollama
EMBEDDING_URL=http://host.docker.internal:11434
EMBEDDING_MODEL=nomic-embed-text
EOF
        echo -e "${GREEN}✓ Created .env for Ollama${NC}"
        echo ""
        echo -e "${YELLOW}⚠ Make sure Ollama is running:${NC}"
        echo "  ollama serve"
        echo "  ollama pull llama3:8b"
        echo "  ollama pull nomic-embed-text"
    else
        echo ""
        read -p "LLM API Key (Cerebras/OpenAI): " llm_key
        read -p "Embedding API Key (OpenRouter/OpenAI): " embed_key
        
        cat > .env << EOF
# Helixir Configuration

# LLM Provider (for extraction & reasoning)
LLM_PROVIDER=cerebras
LLM_MODEL=llama-3.3-70b
LLM_API_KEY=${llm_key}

# Embedding Provider (for semantic search)
EMBEDDING_PROVIDER=openai
EMBEDDING_URL=https://openrouter.ai/api/v1
EMBEDDING_MODEL=sentence-transformers/all-mpnet-base-v2
EMBEDDING_API_KEY=${embed_key}
EOF
        echo -e "${GREEN}✓ Created .env${NC}"
    fi
}

# Start services
start_services() {
    echo ""
    echo -e "${BLUE}🚀 Starting services...${NC}"
    
    $COMPOSE_CMD up -d
    
    echo ""
    echo -e "${GREEN}✓ Services started${NC}"
}

# Wait for HelixDB
wait_for_helix() {
    echo ""
    echo -n "Waiting for HelixDB"
    for i in {1..30}; do
        if curl -s http://localhost:6969/health &> /dev/null; then
            echo ""
            echo -e "${GREEN}✓ HelixDB ready${NC}"
            return
        fi
        echo -n "."
        sleep 1
    done
    echo ""
    echo -e "${YELLOW}⚠ HelixDB taking longer than expected${NC}"
}

# Print success
print_success() {
    echo ""
    echo -e "${GREEN}════════════════════════════════════════════${NC}"
    echo -e "${GREEN}  ✅ Helixir is ready!${NC}"
    echo -e "${GREEN}════════════════════════════════════════════${NC}"
    echo ""
    echo "Next steps:"
    echo ""
    echo "1. Add to Cursor (~/.cursor/mcp.json):"
    echo ""
    echo '   {
     "mcpServers": {
       "helixir": {
         "command": "'$(pwd)'/helixir-mcp",
         "env": {
           "HELIX_HOST": "localhost",
           "HELIX_PORT": "6969"
         }
       }
     }
   }'
    echo ""
    echo "2. Restart Cursor to load the MCP server"
    echo ""
    echo "3. Try: \"Remember that I prefer dark mode\""
    echo ""
    echo -e "${BLUE}Useful commands:${NC}"
    echo "  $COMPOSE_CMD logs -f    # View logs"
    echo "  $COMPOSE_CMD down       # Stop services"
    echo "  $COMPOSE_CMD restart    # Restart"
    echo ""
}

# Main
main() {
    check_docker
    check_compose
    create_env
    start_services
    wait_for_helix
    print_success
}

main "$@"

