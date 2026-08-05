.PHONY: build test test-e2e-hive check run deploy-schema setup onboard doctor config docker-up docker-down migrate-helix-fresh clean help

CARGO      := cargo
BINARY_DIR := helixir/target/release
MCP_BIN    := $(BINARY_DIR)/helixir-mcp
DEPLOY_BIN := $(BINARY_DIR)/helixir-deploy
SCHEMA_DIR := helixir/schema
SKILLS_DIR := helixir/skills
VERSION    ?= $(shell awk -F '"' '/^version[[:space:]]*=/ {print $$2; exit}' helixir/Cargo.toml)
INSTALL_ROOT ?= $(HOME)/.helixir
ifndef INSTALL_ID
INSTALL_ID := $(VERSION)-source-$(shell date -u +%Y%m%d%H%M%S)
endif
INSTALL_VERSION_DIR := $(INSTALL_ROOT)/versions/$(INSTALL_ID)
HELIX_HOST ?= localhost
HELIX_PORT ?= 6969
ONBOARD_ARGS ?=
NON_INTERACTIVE ?= 0
ONBOARD_FLAGS := $(ONBOARD_ARGS)
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
RUNTIME_RPATH := -C link-arg=-Wl,-rpath,@loader_path
else ifeq ($(UNAME_S),Linux)
RUNTIME_RPATH := -C link-arg=-Wl,-rpath,\$$ORIGIN
endif
ifeq ($(NON_INTERACTIVE),1)
ONBOARD_FLAGS += --non-interactive
endif

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

build: ## Build release binaries
	cd helixir && RUSTFLAGS="$(RUSTFLAGS) $(RUNTIME_RPATH)" $(CARGO) build --release

install: build ## Install versioned binaries/assets and run guided onboarding
	@set -eu; \
	if [ -e "$(INSTALL_ROOT)/current" ] && [ ! -L "$(INSTALL_ROOT)/current" ]; then \
		echo "refusing to replace non-symlink $(INSTALL_ROOT)/current" >&2; exit 1; \
	fi; \
	previous_current=""; \
	if [ -L "$(INSTALL_ROOT)/current" ]; then previous_current=$$(readlink "$(INSTALL_ROOT)/current"); fi; \
	mkdir -p "$(INSTALL_VERSION_DIR)/schema" "$(INSTALL_VERSION_DIR)/skills/helixir-memory" "$(INSTALL_ROOT)/bin"; \
	install -m755 "$(BINARY_DIR)/helixir-mcp" "$(INSTALL_VERSION_DIR)/helixir-mcp"; \
	install -m755 "$(BINARY_DIR)/helixir" "$(INSTALL_VERSION_DIR)/helixir"; \
	install -m755 "$(DEPLOY_BIN)" "$(INSTALL_VERSION_DIR)/helixir-deploy"; \
	for runtime_lib in "$(BINARY_DIR)"/libonnxruntime*.dylib "$(BINARY_DIR)"/libonnxruntime*.so*; do \
		[ -e "$$runtime_lib" ] || continue; \
		cp -p "$$runtime_lib" "$(INSTALL_VERSION_DIR)/"; \
	done; \
	install -m644 "$(SCHEMA_DIR)/schema.hx" "$(INSTALL_VERSION_DIR)/schema/schema.hx"; \
	install -m644 "$(SCHEMA_DIR)/queries.hx" "$(INSTALL_VERSION_DIR)/schema/queries.hx"; \
	install -m644 "helixir/helix.toml" "$(INSTALL_VERSION_DIR)/helix.toml"; \
	install -m644 "$(SKILLS_DIR)/helixir-memory/SKILL.md" "$(INSTALL_VERSION_DIR)/skills/helixir-memory/SKILL.md"; \
	ln -sfn "$(INSTALL_VERSION_DIR)" "$(INSTALL_ROOT)/current"; \
	ln -sfn "$(INSTALL_ROOT)/current/helixir" "$(INSTALL_ROOT)/bin/helixir"; \
	ln -sfn "$(INSTALL_ROOT)/current/helixir-mcp" "$(INSTALL_ROOT)/bin/helixir-mcp"; \
	ln -sfn "$(INSTALL_ROOT)/current/helixir-deploy" "$(INSTALL_ROOT)/bin/helixir-deploy"; \
	printf '%s\n' 'installed: $(INSTALL_ROOT)/current (build $(INSTALL_ID))'; \
	if ! "$(INSTALL_ROOT)/current/helixir" onboard $(ONBOARD_FLAGS); then \
		if [ -n "$$previous_current" ]; then \
			ln -sfn "$$previous_current" "$(INSTALL_ROOT)/current"; \
		else \
			rm -f "$(INSTALL_ROOT)/current"; \
		fi; \
		echo 'onboarding failed; restored the previous current pointer' >&2; \
		exit 1; \
	fi

onboard: ## Run the interactive onboarding orchestrator
	"$(INSTALL_ROOT)/bin/helixir" onboard $(ONBOARD_ARGS)

doctor: ## Run the read-only installation doctor
	"$(INSTALL_ROOT)/bin/helixir" doctor

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

mem-reclaim: ## Shed reclaimable page cache charged to the HelixDB container (#89)
	python3 tools/memprobe.py helix-helixir-local-bench_app --reclaim

mem-probe: ## Profile where the container's memory actually goes (#89)
	python3 tools/memprobe.py helix-helixir-local-bench_app

# Lifecycle note: docker-up runs with --restart unless-stopped (same policy as
# docker-compose.yml) — the container auto-recovers from Docker Desktop
# restarts and host reboots; `make docker-down` (or docker stop) is the ONLY
# intended way to keep it down. Both paths configure identical persistence
# (HELIX_DATA_DIR volume) and memory caps.
docker-up: ## Start HelixDB container
	@if docker ps --format '{{.Names}}' | grep -q '^helixdb$$'; then \
		echo "  HelixDB already running"; \
	else \
		docker run -d --name helixdb \
			-p $(HELIX_PORT):$(HELIX_PORT) \
			-v helixdb_data:/data \
			-e HELIX_PORT=$(HELIX_PORT) \
			-e HELIX_DATA_DIR=/data \
			-e HELIX_CORES_OVERRIDE=1 \
			-e MIMALLOC_PURGE_DELAY=0 \
			-e MIMALLOC_PURGE_DECOMMITS=1 \
			-e MIMALLOC_ARENA_PURGE_MULT=1 \
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
