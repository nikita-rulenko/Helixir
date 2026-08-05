# v0.14.1 — The Compatible Judge

This patch makes the mandatory local NLI judge portable across every published
Helixir target. It contains no schema, RBAC, configuration, prompt, or tool
contract changes from v0.14.0.

Tracked by [#132](https://github.com/nikita-rulenko/Helixir/issues/132).

## macOS NLI compatibility

The `ort` Rust binding previously enabled its default API 24 contract while the
official universal macOS runtime packaged by Helixir is ONNX Runtime 1.23.2.
That mismatch passed compilation and MCP startup but failed when NLI opened a
session. Helixir now pins `ort` 2.0.0-rc.12 exactly, disables implicit features,
and explicitly targets API 23. The release can therefore keep one official
universal2 runtime for both Apple Silicon and Intel macOS.

A regression test fixes API 23 as part of the NLI safety contract. Raising it
requires publishing and testing a matching universal runtime first.

## CI compatibility

Seven Rust 1.97 Clippy findings are resolved without lint suppression: periodic
scheduling uses `is_multiple_of`, integer-key ordering uses `sort_by_key`, and
four enums derive their existing default variants explicitly. Behavior is
unchanged, and the Rust 1.88 minimum-supported-version check remains intact.

## Upgrading

This is a binary-only patch. Existing v0.14.0 deployments keep the same HelixDB
v2.3.5 schema and persistent volume; no backup or schema deployment is required
for this patch. Replace the three binaries together with the packaged ONNX
Runtime library, run `helixir doctor --json`, and restart long-lived MCP clients.
Deployments upgrading from v0.13.x must first follow the one-way v0.14.0 RBAC
transition in `UPGRADING.md`.

## Verification

- 267 library tests, 17 CLI tests, and the repository-wide module-budget test.
- Formatting, Rust 1.97 all-target Clippy with warnings denied, Rust 1.88 MSRV
  compilation, rustdoc, and doc tests.
- Mandatory NLI model liveness against the packaged ONNX Runtime 1.23.2 on
  macOS, plus MCP protocol and release-archive smoke checks.
