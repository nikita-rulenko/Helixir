.PHONY: build test test-e2e-hive check run deploy-schema setup config docker-up docker-down migrate-helix-fresh clean help

CARGO      := cargo
BINARY_DIR := helixir/target/release
MCP_BIN    := $(BINARY_DIR)/helixir-mcp
DEPLOY_BIN := $(BINARY_DIR)/helixir-deploy
SCHEMA_DIR := helixir/schema
HELIX_HOST ?= localhost
HELIX_PORT ?= 6969

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

build: ## Build release binaries
	cd helixir && $(CARGO) build --release

install: build ## Install binaries to ~/.helixir/bin (what agent configs should point at)
	@# Agents must NOT execute target/release/ directly: cargo replaces those
	@# files on every rebuild, and macOS can SIGKILL a RUNNING process whose
	@# backing executable changed (observed live: a zeroclaw session's MCP died
	@# mid-conversation minutes after a rebuild). `install` copies to a stable
	@# path; rebuilds never touch what agents are running until you re-install.
	mkdir -p $(HOME)/.helixir/bin
	install -m755 helixir/target/release/helixir-mcp $(HOME)/.helixir/bin/helixir-mcp
	install -m755 helixir/target/release/helixir $(HOME)/.helixir/bin/helixir
	mkdir -p $(HOME)/.local/bin && ln -sf $(HOME)/.helixir/bin/helixir $(HOME)/.local/bin/helixir
	@echo "installed: ~/.helixir/bin/{helixir,helixir-mcp}; point MCP configs at ~/.helixir/bin/helixir-mcp"

test: ## Run all tests
	cd helixir && $(CARGO) test

test-e2e-hive: ## Hive cross-user E2E (needs live HelixDB + LLM + embeddings; same env as MCP)
	cd helixir && HELIX_E2E=1 $(CARGO) test hive_cross_user_collective_link_e2e --test hive_memory_e2e -- --ignored --nocapture

check: ## Run cargo check + clippy
	cd helixir && $(CARGO) check && $(CARGO) clippy

run: ## Run MCP server (debug mode)
	cd helixir && RUST_LOG=helixir=debug $(CARGO) run --bin helixir-mcp

deploy-schema: ## Deploy schema to running HelixDB
	$(DEPLOY_BIN) --host $(HELIX_HOST) --port $(HELIX_PORT) --schema-dir $(SCHEMA_DIR)

setup: docker-up deploy-schema ## Start HelixDB + deploy schema
	@echo "\n  HelixDB running on $(HELIX_HOST):$(HELIX_PORT), schema deployed.\n"

config: ## Print MCP config for Cursor
	@echo '{'
	@echo '  "mcpServers": {'
	@echo '    "helixir": {'
	@echo '      "command": "$(CURDIR)/$(MCP_BIN)",'
	@echo '      "env": {'
	@echo '        "HELIX_HOST": "$(HELIX_HOST)",'
	@echo '        "HELIX_PORT": "$(HELIX_PORT)",'
	@echo '        "HELIX_LLM_PROVIDER": "cerebras",'
	@echo '        "HELIX_LLM_MODEL": "gpt-oss-120b",'
	@echo '        "HELIX_LLM_API_KEY": "YOUR_API_KEY",'
	@echo '        "HELIX_EMBEDDING_PROVIDER": "openai",'
	@echo '        "HELIX_EMBEDDING_MODEL": "nomic-embed-text-v1.5",'
	@echo '        "HELIX_EMBEDDING_URL": "https://openrouter.ai/api/v1",'
	@echo '        "HELIX_EMBEDDING_API_KEY": "YOUR_API_KEY"'
	@echo '      }'
	@echo '    }'
	@echo '  }'
	@echo '}'

docker-up: ## Start HelixDB container
	@if docker ps --format '{{.Names}}' | grep -q '^helixdb$$'; then \
		echo "  HelixDB already running"; \
	else \
		docker run -d --name helixdb \
			-p $(HELIX_PORT):$(HELIX_PORT) \
			-v helixdb_data:/data \
			-e HELIX_PORT=$(HELIX_PORT) \
			-e HELIX_DATA_DIR=/data \
			--restart unless-stopped \
			-m 3g --memory-swap 3g \
			helix-helixir-dev:latest 2>/dev/null || \
		docker start helixdb; \
		echo "  HelixDB started on port $(HELIX_PORT)"; \
	fi

docker-down: ## Stop HelixDB container
	docker stop helixdb 2>/dev/null || true

migrate-helix-fresh: ## Archive helixdb_data volume to .helix-archives/, wipe volume (DESTRUCTIVE)
	@set -e; \
	STAMP=$$(date +%Y%m%d-%H%M%S); \
	ARCH="$(CURDIR)/.helix-archives/helixdb-helixdb_data-$${STAMP}.tar.gz"; \
	mkdir -p "$(CURDIR)/.helix-archives"; \
	if docker ps -a --format '{{.Names}}' | grep -qx helixdb; then \
		docker stop helixdb || true; \
		docker rm helixdb || true; \
	fi; \
	if docker volume inspect helixdb_data >/dev/null 2>&1; then \
		echo "Archiving volume helixdb_data -> $$(basename $$ARCH) ..."; \
		docker run --rm \
			-v helixdb_data:/v:ro \
			-v "$(CURDIR)/.helix-archives:/out" \
			alpine \
			tar czf "/out/$$(basename $$ARCH)" -C /v .; \
		docker volume rm helixdb_data; \
	else \
		echo "Volume helixdb_data does not exist (nothing to archive)."; \
	fi; \
	docker volume create helixdb_data; \
	echo ""; \
	echo "Done. Next: make docker-up && make deploy-schema   OR   helix dockerdev run (repo-root helix.toml)"; \
	echo "MCP: HELIXIR_RETRIEVAL_PROFILE=algo_opt for native BM25 hybrid when Helix has bm25=true."

docker-compose-up: ## Start full stack via docker-compose
	cd helixir && docker compose up -d

docker-compose-down: ## Stop full docker-compose stack
	cd helixir && docker compose down

clean: ## Remove build artifacts
	cd helixir && $(CARGO) clean
