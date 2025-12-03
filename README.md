<p align="center">
  <img src="helixir-logo.jpeg" alt="Helixir Logo" width="400"/>
</p>

<h1 align="center">🧠 Helixir</h1>

<p align="center">
  <strong>The Fastest Memory for LLM Agents</strong><br/>
  <em>Ontological memory framework for AI assistants</em>
</p>

<p align="center">
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-features">Features</a> •
  <a href="#-mcp-integration">MCP Integration</a> •
  <a href="#-configuration">Configuration</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.75+-orange.svg" alt="Rust"/>
  <img src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg" alt="License"/>
  <img src="https://img.shields.io/badge/MCP-compatible-green.svg" alt="MCP"/>
</p>

---

## What is Helixir?

**Helixir** is an associative & causal AI memory framework — the fastest way to give your AI agents persistent, structured, reasoning-capable memory.

It gives your AI agents **persistent, structured, reasoning-capable memory**. Instead of losing context between sessions, your AI remembers facts, learns preferences, tracks goals, and builds knowledge over time.

Built on [HelixDB](https://github.com/HelixDB/helix-db) graph-vector database with native [MCP](https://spec.modelcontextprotocol.io/) support for seamless integration with **Cursor**, **Claude Desktop**, and other AI assistants.

### ⚡ Recommended Stack: Cerebras + OpenRouter

For **maximum speed**, use:
- **[Cerebras](https://cloud.cerebras.ai)** for LLM inference — 70x faster than GPU, free tier available
- **[OpenRouter](https://openrouter.ai)** for embeddings — cheap, reliable, many models

This combination delivers **sub-second memory operations** with the 70B parameter Llama 3.3 model.

### 🦀 Why Rust?

- ⚡ **~50ms startup** — instant response
- 📦 **~15MB memory** — lightweight footprint
- 🎯 **Single binary** — zero runtime dependencies
- 🛡️ **Memory safe** — no crashes, no leaks

---

## ✨ Features

- **🔬 Atomic Fact Extraction** — LLM-powered decomposition into atomic facts
- **🧹 Smart Deduplication** — ADD / UPDATE / SUPERSEDE / NOOP decision engine  
- **🕸️ Graph Memory** — Entities, relations, and reasoning chains
- **🔍 Semantic Search** — Vector similarity + graph traversal (SmartTraversalV2)
- **⏰ Temporal Filtering** — recent (4h), contextual (30d), deep (90d), full
- **🏷️ Ontology Mapping** — skill, preference, goal, fact, opinion, experience, achievement
- **📡 MCP Server** — Native integration with AI assistants
- **🧩 Semantic Chunking** — Automatic splitting of long texts
- **🧠 FastThink** — In-memory working memory for complex reasoning (scratchpad)
- **🎯 Cognitive Protocol** — Built-in triggers and filters that shape AI behavior

---

## 🎯 Cognitive Protocol

Helixir is more than memory storage — it actively shapes how your AI thinks.

### Automatic Recall Triggers

The AI automatically recalls context when it detects patterns in your message:

| You say | AI does |
|---------|---------|
| "remember", "recall" | Searches recent memory |
| "we discussed", "last time" | Deep search in history |
| "why did we" | Retrieves reasoning chains |
| "what's next", "plan" | Recalls task context |
| "like before" | Looks up preferences |

### Importance Filter

Not everything should be saved. Built-in heuristics keep memory clean:

| Save | Skip |
|------|------|
| Decisions, outcomes | Search/grep results |
| Architecture details | Compiler output |
| Errors and fixes | Temporary debug data |
| User preferences | Duplicate information |

### The Result

Your AI develops consistent habits: recalls context at session start, saves important decisions, uses structured reasoning for complex problems, and builds knowledge over time.

---

## 🚀 Quick Start

### One-Command Setup (Docker)

```bash
# Clone and start everything
git clone https://github.com/nikita-rulenko/Helixir
cd helixir

# Create config
cat > .env << 'EOF'
HELIX_LLM_API_KEY=your_cerebras_or_openai_key
HELIX_EMBEDDING_API_KEY=your_openrouter_or_openai_key
EOF

# Start HelixDB + deploy schema
docker-compose up -d
```

**Requirements:**
- Docker & Docker Compose installed
- API keys (see [Configuration](#-configuration))

### Manual Installation

```bash
# 1. Download binary for your platform
# Linux x86_64
curl -L https://github.com/nikita-rulenko/Helixir/releases/latest/download/helixir-linux-x86_64.tar.gz | tar xz

# macOS Apple Silicon
curl -L https://github.com/nikita-rulenko/Helixir/releases/latest/download/helixir-macos-arm64.tar.gz | tar xz

# macOS Intel
curl -L https://github.com/nikita-rulenko/Helixir/releases/latest/download/helixir-macos-x86_64.tar.gz | tar xz

# 2. Start HelixDB (if not running)
docker run -d -p 6969:6969 helixdb/helixdb:latest

# 3. Deploy schema
./helixir-deploy --host localhost --port 6969

# 4. Run MCP server
export LLM_API_KEY=your_key
export EMBEDDING_API_KEY=your_key
./helixir-mcp
```

### Build from Source

```bash
git clone https://github.com/nikita-rulenko/Helixir
cd helixir

# Build
cargo build --release

# Deploy schema & run
./target/release/helixir-deploy --host localhost --port 6969
./target/release/helixir-mcp
```

---

## 🔧 MCP Integration

### Cursor IDE

Edit `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "helixir": {
      "command": "/path/to/helixir-mcp",
      "env": {
        "HELIX_HOST": "localhost",
        "HELIX_PORT": "6969",
        "HELIX_LLM_PROVIDER": "cerebras",
        "HELIX_LLM_MODEL": "llama-3.3-70b",
        "HELIX_LLM_API_KEY": "YOUR_API_KEY",
        "HELIX_EMBEDDING_PROVIDER": "openai",
        "HELIX_EMBEDDING_URL": "https://openrouter.ai/api/v1",
        "HELIX_EMBEDDING_API_KEY": "YOUR_API_KEY"
      }
    }
  }
}
```

### Claude Desktop

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`  
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "helixir": {
      "command": "/path/to/helixir-mcp",
      "env": {
        "HELIX_HOST": "localhost",
        "HELIX_PORT": "6969",
        "HELIX_LLM_API_KEY": "YOUR_API_KEY",
        "HELIX_EMBEDDING_API_KEY": "YOUR_API_KEY"
      }
    }
  }
}
```

### Cursor Rules (Important!)

To make your AI assistant actually USE the memory, add these rules to **Cursor Settings → Rules**:

```
# Core Memory Behavior
- At conversation start, call search_memory to recall relevant context
- Always use Helixir MCP first to recall context about the current project
- After completing tasks, save key outcomes with add_memory
- After reaching context window limit (when Cursor summarizes), read your role and goals from memory

# Search Strategy
- For memory search, use appropriate mode:
  - "recent" for quick context (last 4 hours)
  - "contextual" for balanced search (30 days)
  - "deep" for thorough search (90 days)
  - "full" for complete history
- Use search_by_concept for skill/preference/goal queries
- Use search_reasoning_chain for "why" questions and logical connections

# FastThink for Complex Reasoning
- Before major decisions, use FastThink to structure your reasoning
- Flow: think_start → think_add (multiple thoughts) → think_recall (get context) → think_conclude → think_commit
- Use think_recall to pull relevant facts from main memory into your thinking session
- If session times out, partial thoughts are auto-saved — continue with search_incomplete_thoughts

# What to Save
- ALWAYS save: decisions, outcomes, architecture changes, error fixes
- NEVER save: grep results, lint output, temporary data
```

---

## 📚 MCP Tools

### Memory Operations

| Tool | Description |
|------|-------------|
| `add_memory` | Add memory with LLM extraction → `{memories_added, entities, relations, chunks_created}` |
| `search_memory` | Smart search: `recent` (4h), `contextual` (30d), `deep` (90d), `full` |
| `search_by_concept` | Filter by type: `skill`, `goal`, `preference`, `fact`, `opinion`, `experience`, `achievement` |
| `search_reasoning_chain` | Find logical connections: `IMPLIES`, `BECAUSE`, `CONTRADICTS` |
| `get_memory_graph` | Visualize memory as nodes and edges |
| `update_memory` | Update existing memory content |

### FastThink (Working Memory)

| Tool | Description |
|------|-------------|
| `think_start` | Start isolated thinking session → `{session_id, root_thought_idx}` |
| `think_add` | Add thought to session → `{thought_idx, thought_count, depth}` |
| `think_recall` | Recall facts from main memory (read-only) → `{recalled_count, thought_indices}` |
| `think_conclude` | Mark conclusion → `{conclusion_idx, status: "decided"}` |
| `think_commit` | Save conclusion to main memory → `{memory_id, thoughts_processed}` |
| `think_discard` | Discard session without saving → `{discarded_thoughts}` |
| `think_status` | Get session status → `{thought_count, depth, has_conclusion, elapsed_ms}` |

### Usage Examples

**Store a preference:**
```
"Remember that I prefer dark mode in all applications"
→ add_memory extracts: preference about UI settings
```

**Recall context:**
```
"What do you know about my coding preferences?"
→ search_by_concept(concept_type="preference") 
→ Returns: dark mode preference, editor settings, etc.
```

**Find reasoning chains:**
```
"Why did we decide to use Rust for this project?"
→ search_reasoning_chain(chain_mode="causal")
→ Returns: decision → because → performance requirements
```

**Quick session context:**
```
"What were we working on?"
→ search_memory(mode="recent") 
→ Returns: last 4 hours of activity
```

**Complex reasoning with FastThink:**
```
"Let me think through this architecture decision..."
→ think_start(session_id="arch_decision")
→ think_add("Option A: microservices...")
→ think_add("Option B: monolith...")
→ think_recall("previous architecture decisions")  // pulls from main memory
→ think_conclude("Microservices because of scaling requirements")
→ think_commit()  // saves conclusion to persistent memory
```

---

## 🧠 FastThink (Working Memory)

FastThink provides **isolated scratchpad memory** for complex reasoning tasks. Think of it as a whiteboard that doesn't pollute your main memory until you're ready to commit.

### Why FastThink?

| Problem | Solution |
|---------|----------|
| Thinking out loud pollutes memory | Isolated session, commit only conclusions |
| Need to recall facts while thinking | `think_recall` reads main memory (read-only) |
| Analysis paralysis | Built-in limits: max thoughts, timeout, depth |
| Lost train of thought | Graph structure preserves reasoning chain |

### Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ think_start │ ──▶ │  think_add  │ ──▶ │think_recall │
└─────────────┘     │  (repeat)   │     │ (optional)  │
                    └─────────────┘     └─────────────┘
                           │                   │
                           ▼                   ▼
                    ┌─────────────┐     ┌─────────────┐
                    │think_conclude│ ──▶ │think_commit │
                    └─────────────┘     └─────────────┘
                           │                   │
                           ▼                   ▼
                    ┌─────────────┐     Saved to main
                    │think_discard│     memory as fact
                    └─────────────┘
```

### Limits (configurable)

| Limit | Default | Purpose |
|-------|:-------:|---------|
| `max_thoughts` | 100 | Prevent infinite loops |
| `max_depth` | 10 | Limit reasoning depth |
| `thinking_timeout` | 30s | Prevent stuck sessions |
| `session_ttl` | 5min | Auto-cleanup stale sessions |

### Timeout Recovery

If a session times out, **partial thoughts are automatically saved** to main memory with `[INCOMPLETE]` marker:

```
⏰ Timeout detected
    ↓
📝 Thoughts saved with [INCOMPLETE] marker
    ↓
💾 Stored in main memory
    ↓
🔍 Found at next session start via search_memory("[INCOMPLETE]")
    ↓
🔄 Continue research or dismiss
```

**Recovery flow:**
1. At session start, AI searches for `[INCOMPLETE]` memories
2. If found, offers to continue the research
3. New FastThink session pulls context via `think_recall`
4. After completion, `update_memory` removes `[INCOMPLETE]` marker

No work is lost — incomplete reasoning becomes a starting point for next session.

---

## 📊 Search Modes

| Mode | Time Window | Graph Depth | Use Case |
|------|:-----------:|:-----------:|----------|
| `recent` | 4 hours | 1 | Current session context |
| `contextual` | 30 days | 2 | Balanced (default) |
| `deep` | 90 days | 3 | Thorough historical search |
| `full` | All time | 4 | Complete memory archive |

---

## ⚙️ Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|:--------:|---------|-------------|
| `HELIX_HOST` | ✅ | `localhost` | HelixDB server address |
| `HELIX_PORT` | ✅ | `6969` | HelixDB port |
| `HELIX_LLM_API_KEY` | ✅ | — | API key for LLM provider |
| `HELIX_EMBEDDING_API_KEY` | ✅ | — | API key for embeddings |
| `HELIX_LLM_PROVIDER` | | `cerebras` | `cerebras`, `openai`, `ollama` |
| `HELIX_LLM_MODEL` | | `llama-3.3-70b` | Model name |
| `HELIX_LLM_BASE_URL` | | — | Custom endpoint (Ollama) |
| `HELIX_EMBEDDING_PROVIDER` | | `openai` | `openai`, `ollama` |
| `HELIX_EMBEDDING_URL` | | `https://openrouter.ai/api/v1` | Embedding API URL |
| `HELIX_EMBEDDING_MODEL` | | `all-mpnet-base-v2` | Embedding model |

### Provider Configurations

#### Option 1: Cerebras + OpenRouter (Recommended)

Ultra-fast inference + cheap embeddings:

```bash
HELIX_LLM_PROVIDER=cerebras
HELIX_LLM_MODEL=llama-3.3-70b
HELIX_LLM_API_KEY=csk-xxx              # https://cloud.cerebras.ai

HELIX_EMBEDDING_PROVIDER=openai
HELIX_EMBEDDING_URL=https://openrouter.ai/api/v1
HELIX_EMBEDDING_MODEL=openai/text-embedding-3-large
HELIX_EMBEDDING_API_KEY=sk-or-xxx      # https://openrouter.ai/keys
```

#### Option 2: Fully Local (Ollama)

No API keys, fully private:

```bash
# Install Ollama first: curl -fsSL https://ollama.com/install.sh | sh
ollama pull llama3:8b
ollama pull nomic-embed-text

HELIX_LLM_PROVIDER=ollama
HELIX_LLM_MODEL=llama3:8b
HELIX_LLM_BASE_URL=http://localhost:11434

HELIX_EMBEDDING_PROVIDER=ollama
HELIX_EMBEDDING_URL=http://localhost:11434
HELIX_EMBEDDING_MODEL=nomic-embed-text
```

#### Option 3: OpenAI Only

Simple setup, one API key:

```bash
HELIX_LLM_PROVIDER=openai
HELIX_LLM_MODEL=gpt-4o-mini
HELIX_LLM_API_KEY=sk-xxx

HELIX_EMBEDDING_PROVIDER=openai
HELIX_EMBEDDING_MODEL=text-embedding-3-small
HELIX_EMBEDDING_API_KEY=sk-xxx
```

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      MCP Server (stdio)                      │
├─────────────────────────────────────────────────────────────┤
│                      HelixirClient                           │
├───────────────────────────┬─────────────────────────────────┤
│      ToolingManager       │        FastThinkManager         │
│                           │     (in-memory scratchpad)      │
├──────────┬────────┬───────┼─────────────────────────────────┤
│ LLM      │Decision│Entity │  petgraph::StableDiGraph        │
│ Extractor│ Engine │Manager│  (thoughts, entities, concepts) │
├──────────┼────────┼───────┼─────────────────────────────────┤
│ Reasoning│ Search │Ontology│         ↓ commit               │
│ Engine   │ Engine │Manager │         ↓                      │
├──────────┴────────┴───────┴─────────────────────────────────┤
│                      HelixDB Client                          │
├─────────────────────────────────────────────────────────────┤
│                   HelixDB (graph + vector)                   │
└─────────────────────────────────────────────────────────────┘
```

---

## 🐳 Docker

### Full Stack (HelixDB + Helixir)

```bash
# Start everything
docker-compose up -d

# Check logs
docker-compose logs -f helixir-mcp
```

### Standalone

```bash
# Build
docker build -t helixir .

# Run with external HelixDB
docker run -e HELIX_HOST=your_helixdb_host \
           -e LLM_API_KEY=xxx \
           -e EMBEDDING_API_KEY=xxx \
           helixir
```

---

## 🧪 Development

```bash
# Run tests
cargo test

# Verbose logging
RUST_LOG=helixir=debug cargo run --bin helixir-mcp

# Lint
cargo clippy
cargo fmt --check
```

---

## 📄 License

[AGPL-3.0-or-later](LICENSE)

⚠️ **This is NOT MIT!** If you modify and deploy Helixir as a service, you must open-source your codebase.

---

## 🔗 Links

- [HelixDB](https://github.com/HelixDB/helix-db) — Graph-vector database
- [Helixir-Py](https://github.com/nikita-rulenko/helixir-py) — Python prototype (deprecated)
- [MCP Specification](https://spec.modelcontextprotocol.io/) — Model Context Protocol
- [Cerebras](https://cloud.cerebras.ai) — Fast LLM inference (free tier)
- [OpenRouter](https://openrouter.ai) — Unified LLM/embedding API
