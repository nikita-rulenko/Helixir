# v0.16.0 — Distribution and Stewardship (unreleased)

> **Draft.** This describes locally completed release work. Homebrew/APT
> channels, images and tag are not considered published until the release
> workflow and clean-install gates succeed.

Helixir v0.16.0 makes installation and post-install administration one coherent
product surface while preserving HelixDB as the only memory and RBAC source of
truth.

## Package distribution

- Signed releases feed both `nikita-rulenko/tap` for Homebrew and a dedicated
  signed APT repository for Debian 12 and Ubuntu 22.04+ (`amd64`, `arm64`).
- Formula and Debian packages are derived from the same ABI-gated release
  archives. Container images reuse those native artifacts instead of compiling
  Rust again in Docker or QEMU.
- Package managers own immutable binaries and runtime resources only. Removal
  preserves `~/.helixir`, HelixDB volumes, models, configuration, backups and
  MCP registrations.
- `helixir onboard` remains the single orchestrator after any install method.
  It distinguishes managed-local, existing-local and remote HelixDB, provisions
  mandatory NLI and verified embeddings, registers detected clients and
  converges permanent graph RBAC.

## Shared installer and hardened API

- CLI and browser installation use the same typed
  `detect → prepare → apply → verify` service and native executor modules.
- Long-running browser applies are supervisor-owned, cursor-journaled and
  resumable after browser, container or supervisor interruption.
- The versioned browser API is global-admin-only, origin checked, body bounded,
  `no-store`, secret-safe and intentionally has no CORS surface.
- The web container remains non-root/read-only and receives neither the Docker
  socket nor the host home directory. An authenticated native supervisor owns
  the small allowlisted set of host mutations.

## Stewardship room

- Global admins can inspect a redacted effective configuration and change an
  allowlisted operational subset. Provider credentials are write-only and are
  never returned to the browser or mutation receipt.
- Every patch is reviewed before apply, validated against effective cross-field
  constraints, written atomically with a private backup, and passed to the same
  reload coordinator as `helixir config apply`.
- The managed backup vault lists opaque archive ids rather than filesystem
  paths. Create, verify and restore are disabled for existing-local and remote
  databases.
- Restore requires the exact phrase `RESTORE <archive-id>`, creates a fresh
  safety snapshot first, and probes the real HelixDB schema after recovery. An
  incompatible restore automatically rolls back to the safety snapshot and
  verifies the current schema again.

## Embedding cache identity

- The optional persistent embedding cache is private, byte bounded, locked
  across writers and atomically compacted.
- Its namespace includes cache format, provider, normalized endpoint, model,
  artifact revision, vector dimension and explicit epoch. An identity change
  causes a safe miss instead of reusing vectors from another embedding space.
- Ollama aliases resolve to the installed model digest automatically. Opaque
  remote providers can be invalidated explicitly with
  `HELIXIR_EMBED_CACHE_EPOCH`.
- Persisted keys contain a SHA-256 digest of the exact input; raw memory text is
  not written to the cache.

## Upgrade

There is no HelixDB schema migration from v0.15.0. Upgrade the package or
release binaries, run `helixir onboard`, then `helixir doctor --json`, verify
`helixir control-plane status`, and restart long-lived MCP clients so they load
the current binaries, prompts, resources and skill.
