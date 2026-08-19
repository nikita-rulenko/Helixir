#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' 'usage: check_glibc_baseline.sh --max VERSION FILE [FILE ...]'
}

maximum=''
while (($#)); do
  case "$1" in
    --max) maximum=${2:?}; shift 2; break ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

[[ "$maximum" =~ ^[0-9]+\.[0-9]+$ && $# -gt 0 ]] || { usage >&2; exit 2; }
command -v readelf >/dev/null || { printf '%s\n' 'readelf is required' >&2; exit 1; }
command -v sort >/dev/null || { printf '%s\n' 'sort is required' >&2; exit 1; }

for file in "$@"; do
  [[ -f "$file" ]] || { printf 'missing ELF object: %s\n' "$file" >&2; exit 1; }
  required=$(
    readelf --version-info "$file" 2>/dev/null \
      | grep -Eo 'GLIBC_[0-9]+\.[0-9]+' \
      | sed 's/^GLIBC_//' \
      | sort -Vu \
      | tail -n 1 \
      || true
  )
  [[ -n "$required" ]] || continue
  newest=$(printf '%s\n%s\n' "$maximum" "$required" | sort -V | tail -n 1)
  if [[ "$newest" != "$maximum" ]]; then
    printf '%s requires GLIBC_%s, newer than the supported GLIBC_%s baseline\n' \
      "$file" "$required" "$maximum" >&2
    exit 1
  fi
  printf '%s: GLIBC_%s <= GLIBC_%s\n' "$file" "$required" "$maximum"
done
