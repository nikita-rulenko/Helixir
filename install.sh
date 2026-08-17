#!/usr/bin/env bash
# Helixir one-command bootstrapper.
#
# Source checkout: make build && make install.
# Release host: download the matching checksummed GitHub asset, install it in a
# versioned directory, atomically switch ~/.helixir/current, then run onboard.

set -euo pipefail

REPO="nikita-rulenko/Helixir"
ROOT="${HELIXIR_HOME:-${HOME}/.helixir}"
VERSION="${HELIXIR_VERSION:-latest}"
NON_INTERACTIVE=false
DRY_RUN=false
INSTALL_WEB=true
SOURCE_DIR=""

usage() {
  printf '%s\n' \
    'Usage: install.sh [--version VERSION] [--dir PATH] [--non-interactive] [--dry-run] [--no-web]' \
    '' \
    'If run from a Helixir checkout, this delegates to make install.'
}

while (($#)); do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir) ROOT="$2"; shift 2 ;;
    --non-interactive) NON_INTERACTIVE=true; shift ;;
    --dry-run) DRY_RUN=true; shift ;;
    --no-web) INSTALL_WEB=false; shift ;;
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
  install_web=1
  $INSTALL_WEB || install_web=0
  exec make -C "$SOURCE_DIR" install INSTALL_ROOT="$ROOT" INSTALL_WEB="$install_web" ONBOARD_ARGS="${args[*]-}"
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
base="${HELIXIR_RELEASE_BASE_URL:-$base}"
tmp=$(mktemp -d)
install_stage=""
cleanup() {
  rm -rf "$tmp"
  [[ -z "$install_stage" ]] || rm -rf "$install_stage"
}
trap cleanup EXIT
archive="$tmp/${artifact}.tar.gz"

curl -fL --retry 3 -o "$archive" "${base}/${artifact}.tar.gz"
curl -fL --retry 3 -o "$tmp/SHA256SUMS" "${base}/SHA256SUMS"
expected=$(awk -v name="${artifact}.tar.gz" '$2 == name || $2 == "*" name {print $1}' "$tmp/SHA256SUMS" | head -n1)
[[ -n "$expected" ]] || { echo 'release checksum entry is missing' >&2; exit 1; }
if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$archive" | awk '{print $1}')
elif command -v shasum >/dev/null; then
  actual=$(shasum -a 256 "$archive" | awk '{print $1}')
else
  echo 'sha256sum or shasum is required' >&2
  exit 1
fi
[[ "$actual" == "$expected" ]] || { echo 'release checksum mismatch' >&2; exit 1; }

mkdir -p "${ROOT}/versions" "${ROOT}/bin"
install_stage=$(mktemp -d "${ROOT}/versions/.${VERSION}.XXXXXX")
tar -xzf "$archive" -C "$install_stage"
chmod 755 "$install_stage"/helixir "$install_stage"/helixir-mcp "$install_stage"/helixir-deploy
version_dir="${ROOT}/versions/${VERSION}-$(date -u +%Y%m%d%H%M%S)"
mv "$install_stage" "$version_dir"
install_stage=""
if [[ -e "${ROOT}/current" && ! -L "${ROOT}/current" ]]; then
  echo "refusing to replace non-symlink ${ROOT}/current" >&2
  exit 1
fi
previous_current=""
[[ ! -L "${ROOT}/current" ]] || previous_current=$(readlink "${ROOT}/current")
ln -sfn "$version_dir" "${ROOT}/current"
ln -sfn "${ROOT}/current/helixir" "${ROOT}/bin/helixir"
ln -sfn "${ROOT}/current/helixir-mcp" "${ROOT}/bin/helixir-mcp"
ln -sfn "${ROOT}/current/helixir-deploy" "${ROOT}/bin/helixir-deploy"

onboard_args=()
$NON_INTERACTIVE && onboard_args+=(--non-interactive)
$DRY_RUN && onboard_args+=(--dry-run)
if ! "${ROOT}/current/helixir" onboard "${onboard_args[@]}"; then
  if [[ -n "$previous_current" ]]; then
    ln -sfn "$previous_current" "${ROOT}/current"
  else
    rm -f "${ROOT}/current"
  fi
  echo 'onboarding failed; restored the previous current pointer' >&2
  exit 1
fi
if $INSTALL_WEB && ! $DRY_RUN; then
  "${ROOT}/current/helixir" control-plane install
fi
printf 'Helixir %s installed at %s/current\n' "$VERSION" "$ROOT"
