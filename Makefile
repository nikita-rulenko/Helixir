.PHONY: build build-client install-client web-build control-plane-image control-plane-secrets control-plane-supervisor control-plane-up stack-up stack-down docker-compose-up docker-compose-down helixir test test-e2e-manifest test-e2e-live test-e2e-hive test-pre-release-client test-control-plane-soak check run deploy-schema setup onboard doctor config docker-up docker-down migrate-helix-fresh clean help

CARGO      := cargo
BINARY_DIR := helixir/target/release
CLIENT_BINARY := helixir-client/target/release/helixir-client
MCP_BIN    := $(BINARY_DIR)/helixir-mcp
DEPLOY_BIN := $(BINARY_DIR)/helixir-deploy
SCHEMA_DIR := helixir/schema
SKILLS_DIR := helixir/skills
WEB_DIR    := helixir/web
VERSION    ?= $(shell awk -F '"' '/^version[[:space:]]*=/ {print $$2; exit}' helixir/Cargo.toml)
CLIENT_GATE_ARCHIVE ?=
CLIENT_GATE_CLIENT_ARCHIVE ?=
CLIENT_GATE_ARCH ?= $(if $(filter arm64 aarch64,$(shell uname -m)),arm64,amd64)
CONTROL_PLANE_IMAGE ?= helixir-control-plane:$(VERSION)
CONTROL_PLANE_TOKEN_FILE ?= $(HOME)/.helixir/run/control-plane-browser.token
INSTALL_ROOT ?= $(HOME)/.helixir
ifndef INSTALL_ID
INSTALL_ID := $(VERSION)-source-$(shell date -u +%Y%m%d%H%M%S)
endif
INSTALL_VERSION_DIR := $(INSTALL_ROOT)/versions/$(INSTALL_ID)
HELIX_HOST ?= localhost
HELIX_PORT ?= 6969
ONBOARD_ARGS ?=
NON_INTERACTIVE ?= 0
INSTALL_WEB ?= $(if $(filter 1,$(NON_INTERACTIVE)),0,1)
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
ifeq ($(INSTALL_WEB),1)
INSTALL_DEPS := build control-plane-image
else
INSTALL_DEPS := build
endif

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

web-build: ## Build the HTML5/Tailwind control plane
	cd $(WEB_DIR) && npm ci && npm run build

build: ## Build native release binaries for this host
	cd helixir && RUSTFLAGS="$(RUSTFLAGS) $(RUNTIME_RPATH)" $(CARGO) build --release

build-client: ## Build the thin remote-agent client only
	cd helixir-client && $(CARGO) build --release --locked

install-client: build-client ## Install the thin client binary and start guided connection
	@mkdir -p "$(INSTALL_ROOT)/bin"
	install -m755 "$(CLIENT_BINARY)" "$(INSTALL_ROOT)/bin/helixir-client"
	"$(INSTALL_ROOT)/bin/helixir-client" connect $(CLIENT_ARGS)

control-plane-image: ## Build the isolated web frontend/backend image
	docker build --target control-plane --tag "$(CONTROL_PLANE_IMAGE)" helixir

control-plane-secrets: build ## Initialize private control-plane credentials
	"$(BINARY_DIR)/helixir" web --prepare-token --no-open --token-file "$(CONTROL_PLANE_TOKEN_FILE)"

control-plane-supervisor: build ## Run the authenticated native host bridge
	"$(BINARY_DIR)/helixir" supervisor

helixir: build ## Compatibility alias: `make helixir`

install: $(INSTALL_DEPS) ## Install native components and run guided onboarding
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
	fi; \
	if [ "$(INSTALL_WEB)" = "1" ]; then \
		HELIXIR_CONTROL_PLANE_IMAGE="$(CONTROL_PLANE_IMAGE)" "$(INSTALL_ROOT)/current/helixir" control-plane install; \
	fi

onboard: ## Run the interactive onboarding orchestrator
	"$(INSTALL_ROOT)/bin/helixir" onboard $(ONBOARD_ARGS)

doctor: ## Run the read-only installation doctor
	"$(INSTALL_ROOT)/bin/helixir" doctor

test: ## Run all tests
	cd helixir && $(CARGO) test
	cd helixir-client && $(CARGO) test --locked

test-e2e-manifest: ## Deterministic ignored-E2E inventory and environment ownership check
	python3 tools/e2e_matrix.py --check
	cd tools && python3 -m unittest -v test_e2e_matrix.py

test-e2e-live: ## Canonical disposable current-schema E2E matrix (never production port 6970)
	python3 tools/e2e_matrix.py --run --topology current-schema

test-e2e-hive: ## Hive cross-user E2E (needs live HelixDB + LLM + embeddings; same env as MCP)
	cd helixir && HELIX_E2E=1 $(CARGO) test hive_cross_user_collective_link_e2e --test hive_memory_e2e -- --ignored --nocapture

test-pre-release-client: ## Disposable APT, two-client and RBAC visibility release gate
	@if [ "$${HELIXIR_CLIENT_GATE_DISPOSABLE_DOCKER:-0}" != 1 ]; then \
		echo 'run this gate in a disposable VM/CI Docker daemon; set HELIXIR_CLIENT_GATE_DISPOSABLE_DOCKER=1 there' >&2; exit 2; \
	fi
	@test -f "$(CLIENT_GATE_ARCHIVE)" || { \
		echo 'set CLIENT_GATE_ARCHIVE to a Linux release archive' >&2; exit 2; \
	}
	@test -f "$(CLIENT_GATE_CLIENT_ARCHIVE)" || { \
		echo 'set CLIENT_GATE_CLIENT_ARCHIVE to a Linux client release archive' >&2; exit 2; \
	}
	tools/pre_release_client_gate.sh --archive "$(CLIENT_GATE_ARCHIVE)" \
		--client-archive "$(CLIENT_GATE_CLIENT_ARCHIVE)" \
		--version "$(VERSION)" --arch "$(CLIENT_GATE_ARCH)"

test-pre-release-client-preflight: ## Deterministic safety tests for the Docker gate
	tools/test_pre_release_client_gate_preflight.sh

test-control-plane-soak: ## Bounded live polling soak (requires running control-plane)
	python3 tools/control_plane_soak.py

check: ## Run cargo check + clippy
	cd helixir && $(CARGO) check && $(CARGO) clippy
	cd helixir-client && $(CARGO) check --locked && $(CARGO) clippy --all-targets -- -D warnings

run: ## Run MCP server (debug mode)
	cd helixir && RUST_LOG=helixir=debug $(CARGO) run --bin helixir-mcp

deploy-schema: ## Deploy schema to running HelixDB
	$(DEPLOY_BIN) --host $(HELIX_HOST) --port $(HELIX_PORT) --schema-dir $(SCHEMA_DIR)

setup: docker-up deploy-schema ## Start HelixDB + deploy schema
	@echo "\n  HelixDB running on $(HELIX_HOST):$(HELIX_PORT), schema deployed.\n"

control-plane-up: control-plane-secrets control-plane-image ## Start the isolated admin UI and its managed HelixDB
	HELIXIR_CONTROL_PLANE_IMAGE="$(CONTROL_PLANE_IMAGE)" HELIXIR_CONTROL_PLANE_TOKEN_SOURCE="$(CONTROL_PLANE_TOKEN_FILE)" docker compose -f helixir/docker-compose.yml up -d control-plane
	@token=$$(tr -d '\r\n' < "$(CONTROL_PLANE_TOKEN_FILE)"); printf 'Helixir web control plane: http://127.0.0.1:%s/#token=%s\n' "$${HELIXIR_WEB_PORT:-6971}" "$$token"

stack-up: control-plane-secrets control-plane-image ## Start the managed HelixDB + admin control plane stack
	HELIXIR_CONTROL_PLANE_IMAGE="$(CONTROL_PLANE_IMAGE)" HELIXIR_CONTROL_PLANE_TOKEN_SOURCE="$(CONTROL_PLANE_TOKEN_FILE)" docker compose -f helixir/docker-compose.yml up -d
	@token=$$(tr -d '\r\n' < "$(CONTROL_PLANE_TOKEN_FILE)"); printf 'Helixir web control plane: http://127.0.0.1:%s/#token=%s\n' "$${HELIXIR_WEB_PORT:-6971}" "$$token"

stack-down: ## Stop the managed stack without deleting its volume
	docker compose -f helixir/docker-compose.yml down

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

docker-compose-up: stack-up ## Compatibility alias for the managed stack

docker-compose-down: stack-down ## Compatibility alias for stopping the managed stack

clean: ## Remove build artifacts
	cd helixir && $(CARGO) clean
	cd helixir-client && $(CARGO) clean
