#!/usr/bin/env bash
# Helixir one-command bootstrapper.
#
# Source checkout: make build && make install.
# Release host: download the matching signed GitHub asset, install it in a
# versioned directory, atomically switch ~/.helixir/current, then run onboard.

set -euo pipefail

REPO="nikita-rulenko/Helixir"
ROOT="${HELIXIR_HOME:-${HOME}/.helixir}"
VERSION="${HELIXIR_VERSION:-latest}"
NON_INTERACTIVE=false
DRY_RUN=false
SOURCE_DIR=""

usage() {
  printf '%s\n' \
    'Usage: install.sh [--version VERSION] [--dir PATH] [--non-interactive] [--dry-run]' \
    '' \
    'If run from a Helixir checkout, this delegates to make install.'
}

while (($#)); do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir) ROOT="$2"; shift 2 ;;
    --non-interactive) NON_INTERACTIVE=true; shift ;;
    --dry-run) DRY_RUN=true; shift ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -f "helixir/Cargo.toml" && -f "Makefile" ]]; then
  SOURCE_DIR="$PWD"
fi

if [[ -n "$SOURCE_DIR" ]]; then
  args=()
  $NON_INTERACTIVE && args+=(--non-interactive)
  $DRY_RUN && args+=(--dry-run)
  exec make -C "$SOURCE_DIR" install ONBOARD_ARGS="${args[*]-}"
fi

command -v curl >/dev/null || { echo 'curl is required' >&2; exit 1; }
command -v tar >/dev/null || { echo 'tar is required' >&2; exit 1; }

os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64) artifact=helixir-macos-arm64 ;;
  Darwin/x86_64) artifact=helixir-macos-x86_64 ;;
  Linux/x86_64) artifact=helixir-linux-x86_64 ;;
  Linux/aarch64|Linux/arm64) artifact=helixir-linux-arm64 ;;
  *) echo "unsupported platform: $os/$arch" >&2; exit 1 ;;
esac

if [[ "$VERSION" == latest ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"v\([^"]*\)".*/\1/p' | head -n1)
fi
[[ -n "$VERSION" ]] || { echo 'could not determine release version' >&2; exit 1; }

base="https://github.com/${REPO}/releases/download/v${VERSION}"
tmp=$(mktemp -d)
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT
archive="$tmp/${artifact}.tar.gz"

curl -fL --retry 3 -o "$archive" "${base}/${artifact}.tar.gz"
if curl -fL --retry 3 -o "$tmp/SHA256SUMS" "${base}/SHA256SUMS"; then
  expected=$(awk -v name="${artifact}.tar.gz" '$2 == name || $2 == "*" name {print $1}' "$tmp/SHA256SUMS" | head -n1)
  if [[ -n "$expected" ]]; then
    actual=$(shasum -a 256 "$archive" | awk '{print $1}')
    [[ "$actual" == "$expected" ]] || { echo 'release checksum mismatch' >&2; exit 1; }
  fi
fi

version_dir="${ROOT}/versions/${VERSION}"
mkdir -p "$version_dir"
tar -xzf "$archive" -C "$version_dir"
chmod 755 "$version_dir"/helixir "$version_dir"/helixir-mcp "$version_dir"/helixir-deploy
ln -sfn "$version_dir" "${ROOT}/current"
mkdir -p "${ROOT}/bin"
ln -sfn "${ROOT}/current/helixir" "${ROOT}/bin/helixir"
ln -sfn "${ROOT}/current/helixir-mcp" "${ROOT}/bin/helixir-mcp"
ln -sfn "${ROOT}/current/helixir-deploy" "${ROOT}/bin/helixir-deploy"

onboard_args=()
$NON_INTERACTIVE && onboard_args+=(--non-interactive)
$DRY_RUN && onboard_args+=(--dry-run)
"${ROOT}/current/helixir" onboard "${onboard_args[@]}"
printf 'Helixir %s installed at %s/current\n' "$VERSION" "$ROOT"
