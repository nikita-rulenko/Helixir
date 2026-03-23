.PHONY: build test test-e2e-hive check run deploy-schema setup config docker-up docker-down clean help

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
			--restart unless-stopped \
			helixdb/helixdb:latest 2>/dev/null || \
		docker start helixdb; \
		echo "  HelixDB started on port $(HELIX_PORT)"; \
	fi

docker-down: ## Stop HelixDB container
	docker stop helixdb 2>/dev/null || true

docker-compose-up: ## Start full stack via docker-compose
	cd helixir && docker compose up -d

docker-compose-down: ## Stop full docker-compose stack
	cd helixir && docker compose down

clean: ## Remove build artifacts
	cd helixir && $(CARGO) clean
