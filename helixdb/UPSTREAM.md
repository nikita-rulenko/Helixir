# HelixDB v2 maintenance fork

This directory is a source snapshot of
[`HelixDB/helix-db`](https://github.com/HelixDB/helix-db) at commit
`17e7ecf764aecd553e1f54ca25320d654153a9aa`, the code shipped by the pinned
HelixDB CLI v2.3.5 used by Helixir.

HelixDB v2 is retained because the v3 engine and deployment model are not
storage-compatible with Helixir's current graph contract. Local changes must
remain narrowly scoped, profiled against the canonical daemon workload, and
covered by differential tests against the untouched upstream snapshot.

The upstream code is licensed under AGPL-3.0; see [LICENSE](LICENSE). This
license applies to this directory. Helixir remains a separate process and
retains its own license.

## Fork policy

- Keep the upstream commit recorded above current whenever the snapshot is
  refreshed.
- Production builds keep the upstream allocator unless a diagnostic feature
  is selected explicitly.
- Profiling builds are evidence tools and never determine a release verdict.
- Storage migrations are backup-first, transactional, restart-safe, and
  verified on a restored volume before production deployment.
- No change may weaken Helixir's graph/RBAC invariants or silently fall back to
  a scan when an indexed lookup was requested.
