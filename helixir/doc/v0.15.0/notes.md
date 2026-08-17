# v0.15.0 — The Memory Observatory

Helixir v0.15.0 adds a global-admin-only web control plane without weakening
the graph-backed RBAC or spreading frontend files across the host.

## Admin control plane

- A responsive HTML5/Tailwind interface covers installation, live counters,
  users and agents, group/role administration, dedup federations, memory
  exploration, Moirai evidence and Hygieia telemetry.
- The memory field is category-first and bounded. Groups and identities filter
  cached graph snapshots; persisted typed reasoning edges remain inspectable.
- Agent presence follows heartbeat/farewell freshness rather than stale status
  strings, and every overview counter drills into its administrative surface.

## Distribution and recovery

- The compiled SPA and Axum API ship as one immutable
  `helixir-control-plane` image published to GHCR for every release.
- `helixir control-plane install` creates a hardened read-only container and a
  token-authenticated typed native supervisor. launchd or systemd restarts the
  supervisor after login/reboot; Docker restarts the UI container.
- The container receives neither Docker socket, home directory nor host write
  access. Host mutations remain typed, authenticated and journaled.
- `install.sh --no-web` preserves the fully headless CLI flow.

## Installation safety

- Browser installation uses the same deterministic `InstallPlan` and executor
  as the CLI. Apply operations are private, redacted, durable and resumable
  after browser, container or supervisor interruption.
- Managed-local, existing-local and remote HelixDB ownership remain distinct.
- NLI stays mandatory. Ollama/Nomic remains the default and doctor recovery
  path; explicitly configured remote embeddings remain supported.

## Release gates

- Playwright covers Chromium, Firefox, WebKit and mobile layouts, including the
  complete first-run review/apply/verify journey.
- Global-admin authorization, every denied non-admin role, accessibility,
  responsive behavior and bounded polling are automated.
- Every required installer action has failure-injection rollback coverage.
- A live authenticated 100-read soak held the control-plane working set between
  130.2 and 130.7 MiB, well inside the 96 MiB growth budget.

## Upgrade

Back up the HelixDB volume, install v0.15.0 through the normal transactional
flow, and run `helixir doctor`. RBAC remains permanently enabled and no rollback
to the pre-RBAC model is supported. Existing memory and role history are
preserved; the web control plane is an administrative projection, not a second
source of truth.
