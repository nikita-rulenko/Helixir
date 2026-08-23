#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  printf '%s\n' \
    'usage: build_dogfood_candidate.sh --output DIR --sha GIT_SHA [--preflight-only]'
}

output=''
candidate_sha=''
preflight_only=0
while (($#)); do
  case "$1" in
    --output) output=${2:?}; shift 2 ;;
    --sha) candidate_sha=${2:?}; shift 2 ;;
    --preflight-only) preflight_only=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

for command in docker git helix gzip python3 tar; do
  command -v "$command" >/dev/null || {
    printf 'dogfood candidate fallback requires %s\n' "$command" >&2
    exit 1
  }
done

[[ "$(helix --version)" == *'2.3.5'* ]] || {
  printf '%s\n' 'dogfood candidate fallback requires Helix CLI v2.3.5' >&2
  exit 1
}

[[ ${HELIXIR_DOGFOOD_DISPOSABLE_DOCKER:-0} == 1 ]] || {
  printf '%s\n' \
    'refusing to build a dogfood candidate without HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1' >&2
  exit 1
}

docker info >/dev/null 2>&1 || {
  printf '%s\n' 'Docker Engine is unavailable; aborting dogfood candidate build' >&2
  exit 1
}

containers=$(docker ps -aq 2>/dev/null) || {
  printf '%s\n' 'cannot inspect Docker containers; aborting dogfood candidate build' >&2
  exit 1
}
volumes=$(docker volume ls -q 2>/dev/null) || {
  printf '%s\n' 'cannot inspect Docker volumes; aborting dogfood candidate build' >&2
  exit 1
}
if [[ -n "$containers" || -n "$volumes" ]]; then
  printf '%s\n' \
    'refusing to use a non-empty Docker daemon for the dogfood candidate build' >&2
  exit 1
fi

reported_memory_bytes=$(docker info --format '{{.MemTotal}}')
[[ $reported_memory_bytes =~ ^[0-9]+$ ]] || {
  printf '%s\n' 'Docker daemon returned an invalid memory limit' >&2
  exit 1
}
asserted_memory_bytes=${HELIXIR_DOGFOOD_MEMORY_BYTES:-0}
[[ $asserted_memory_bytes =~ ^[0-9]+$ ]] || {
  printf '%s\n' 'HELIXIR_DOGFOOD_MEMORY_BYTES must be an integer' >&2
  exit 1
}
minimum_memory_bytes=$((4 * 1024 * 1024 * 1024))
effective_memory_bytes=$reported_memory_bytes
if ((asserted_memory_bytes < effective_memory_bytes)); then
  effective_memory_bytes=$asserted_memory_bytes
fi
((effective_memory_bytes >= minimum_memory_bytes)) || {
  printf '%s\n' \
    'dogfood candidate build requires HELIXIR_DOGFOOD_MEMORY_BYTES and at least 4 GiB of effective Docker memory' >&2
  exit 1
}

swap_bytes=${HELIXIR_DOGFOOD_SWAP_BYTES:-0}
[[ $swap_bytes =~ ^[0-9]+$ ]] || {
  printf '%s\n' 'HELIXIR_DOGFOOD_SWAP_BYTES must be an integer' >&2
  exit 1
}
minimum_swap_bytes=$((2 * 1024 * 1024 * 1024))
((swap_bytes >= minimum_swap_bytes)) || {
  printf '%s\n' \
    'dogfood candidate build requires at least 2 GiB of explicitly provisioned swap' >&2
  exit 1
}

if ((preflight_only)); then
  printf '%s\n' 'dogfood candidate Docker preflight passed'
  exit 0
fi

[[ -n "$output" && $candidate_sha =~ ^[0-9a-f]{40}$ ]] || {
  usage >&2
  exit 2
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
[[ "$(git -C "$repo_root" rev-parse HEAD)" == "$candidate_sha" ]] || {
  printf '%s\n' 'candidate SHA does not match repository HEAD' >&2
  exit 1
}
git -C "$repo_root" diff --quiet -- . || {
  printf '%s\n' 'tracked worktree changes make the dogfood candidate non-exact' >&2
  exit 1
}
git -C "$repo_root" diff --cached --quiet -- . || {
  printf '%s\n' 'staged changes make the dogfood candidate non-exact' >&2
  exit 1
}

work=$(mktemp -d "${TMPDIR:-/tmp}/helixir-dogfood-build.XXXXXX")
real_docker=$(command -v docker)
cleanup() {
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

# Build the control plane from an archive of the exact candidate commit. The
# fallback must never package a stale binary or web tree left in the ignored
# release directory by an earlier workflow download.
mkdir -p "$work/source"
git -C "$repo_root" archive "$candidate_sha" helixir \
  | tar -x -C "$work/source" --strip-components=1

python3 - "$work/source/Dockerfile" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
builder = "FROM rust:1.88-slim-bookworm AS builder\n"
jobs = "ENV CARGO_BUILD_JOBS=1\n"
release_build = (
    "cargo build --release --bin helixir --bin helixir-mcp --bin helixir-deploy"
)
if builder not in text:
    raise SystemExit("control-plane Dockerfile has no Rust builder stage")
if release_build not in text:
    raise SystemExit("control-plane Dockerfile has no expected release build command")
text = text.replace(builder, builder + "\n" + jobs, 1)
text = text.replace(release_build, "cargo build --release --bin helixir", 1)
path.write_text(text)
PY

cat >"$work/docker" <<'SHIM'
#!/usr/bin/env bash
set -Eeuo pipefail

if [[ ${1:-} == compose && ${*: -1} == build ]]; then
  dockerfile="$PWD/Dockerfile"
  [[ -f "$dockerfile" ]] || {
    printf 'generated HelixDB Dockerfile is missing at %s\n' "$dockerfile" >&2
    exit 1
  }
  python3 - "$dockerfile" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
marker = "FROM chef AS builder\n"
jobs = "ENV CARGO_BUILD_JOBS=1\n"
if jobs not in text:
    if marker not in text:
        raise SystemExit("generated Dockerfile has no builder marker")
    path.write_text(text.replace(marker, marker + jobs, 1))
PY
fi

exec "$HELIXIR_DOGFOOD_REAL_DOCKER" "$@"
SHIM
chmod +x "$work/docker"

printf '%s\n' '[1/4] Compile the exact HelixDB schema on the disposable daemon'
(
  cd "$repo_root/helixir"
  PATH="$work:$PATH" HELIXIR_DOGFOOD_REAL_DOCKER="$real_docker" \
    helix check
  PATH="$work:$PATH" HELIXIR_DOGFOOD_REAL_DOCKER="$real_docker" \
    helix build -i dev --quiet
)

db_image="helix-helixir-candidate:$candidate_sha"
control_plane_image="helixir-control-plane-candidate:$candidate_sha"
docker image inspect helix-helixir-dev:latest >/dev/null
docker tag helix-helixir-dev:latest "$db_image"

printf '%s\n' '[2/4] Package the release-shaped control plane'
docker build --build-arg TARGETARCH=arm64 --target control-plane \
  --tag "$control_plane_image" "$work/source"

for image in "$db_image" "$control_plane_image"; do
  [[ "$(docker image inspect -f '{{.Architecture}}' "$image")" == arm64 ]] || {
    printf 'dogfood image %s is not ARM64\n' "$image" >&2
    exit 1
  }
done

mkdir -p "$output"
db_archive="$output/helixdb-candidate-arm64.tar.gz"
control_plane_archive="$output/control-plane-candidate-arm64.tar.gz"

printf '%s\n' '[3/4] Export exact candidate images'
docker save "$db_image" | gzip -1 >"$db_archive"
docker save "$control_plane_image" | gzip -1 >"$control_plane_archive"
gzip -t "$db_archive"
gzip -t "$control_plane_archive"
[[ -s "$db_archive" && -s "$control_plane_archive" ]]

printf '%s\n' '[4/4] Write checksums'
if command -v sha256sum >/dev/null; then
  (cd "$output" && sha256sum "${db_archive##*/}" "${control_plane_archive##*/}") \
    >"$output/SHA256SUMS"
else
  (cd "$output" && shasum -a 256 "${db_archive##*/}" "${control_plane_archive##*/}") \
    >"$output/SHA256SUMS"
fi

printf 'dogfood candidate archives exported to %s\n' "$output"
