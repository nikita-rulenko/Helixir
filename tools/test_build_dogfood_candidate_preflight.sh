#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/helixir-dogfood-preflight.XXXXXX")
cleanup() {
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

printf '#!/usr/bin/env bash\nprintf "Helix CLI 2.3.5\\n"\n' >"$work/helix"
chmod +x "$work/helix"

write_fake_docker() {
  local memory=$1 containers=${2:-} volumes=${3:-}
  cat >"$work/docker" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
  info)
    if [[ \${2:-} == --format ]]; then printf '%s\\n' '$memory'; fi
    exit 0
    ;;
  ps) printf '%s\\n' '$containers' ;;
  volume)
    [[ \${2:-} == ls ]] && printf '%s\\n' '$volumes'
    ;;
  *) exit 0 ;;
esac
EOF
  chmod +x "$work/docker"
}

four_gib=$((4 * 1024 * 1024 * 1024))
two_gib=$((2 * 1024 * 1024 * 1024))

write_fake_docker "$four_gib"
if PATH="$work:$PATH" HELIXIR_DOGFOOD_SWAP_BYTES="$two_gib" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected missing disposable assertion to fail' >&2
  exit 1
fi
grep -q 'HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1' "$work/err"

write_fake_docker "$((four_gib - 1))"
if PATH="$work:$PATH" HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1 \
  HELIXIR_DOGFOOD_MEMORY_BYTES="$four_gib" \
  HELIXIR_DOGFOOD_SWAP_BYTES="$two_gib" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected insufficient memory to fail' >&2
  exit 1
fi
grep -q 'at least 4 GiB' "$work/err"

write_fake_docker "$four_gib"
if PATH="$work:$PATH" HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1 \
  HELIXIR_DOGFOOD_SWAP_BYTES="$two_gib" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected missing memory assertion to fail' >&2
  exit 1
fi
grep -q 'HELIXIR_DOGFOOD_MEMORY_BYTES' "$work/err"

write_fake_docker "$four_gib"
if PATH="$work:$PATH" HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1 \
  HELIXIR_DOGFOOD_MEMORY_BYTES="$four_gib" \
  HELIXIR_DOGFOOD_SWAP_BYTES="$((two_gib - 1))" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected insufficient swap assertion to fail' >&2
  exit 1
fi
grep -q 'at least 2 GiB' "$work/err"

write_fake_docker "$four_gib" 'existing-container' ''
if PATH="$work:$PATH" HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1 \
  HELIXIR_DOGFOOD_MEMORY_BYTES="$four_gib" \
  HELIXIR_DOGFOOD_SWAP_BYTES="$two_gib" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected non-empty daemon to fail' >&2
  exit 1
fi
grep -q 'non-empty Docker daemon' "$work/err"

write_fake_docker "$four_gib"
PATH="$work:$PATH" HELIXIR_DOGFOOD_DISPOSABLE_DOCKER=1 \
  HELIXIR_DOGFOOD_MEMORY_BYTES="$four_gib" \
  HELIXIR_DOGFOOD_SWAP_BYTES="$two_gib" \
  "$repo_root/tools/build_dogfood_candidate.sh" --preflight-only \
  | grep -q 'preflight passed'

printf '%s\n' 'dogfood candidate preflight tests passed'
