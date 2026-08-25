#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' 'usage: build_deb.sh --archive FILE --version VERSION --arch amd64|arm64 --output DIR'
}

archive=''
version=''
arch=''
output=''
while (($#)); do
  case "$1" in
    --archive) archive=${2:?}; shift 2 ;;
    --version) version=${2:?}; shift 2 ;;
    --arch) arch=${2:?}; shift 2 ;;
    --output) output=${2:?}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

[[ -f "$archive" && -n "$version" && -n "$output" ]] || { usage >&2; exit 2; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+~-][0-9A-Za-z.-]+)?$ ]] || {
  printf 'invalid Debian version: %s\n' "$version" >&2
  exit 2
}
case "$arch" in
  amd64|arm64) ;;
  *) printf 'unsupported Debian architecture: %s\n' "$arch" >&2; exit 2 ;;
esac
command -v dpkg-deb >/dev/null || { printf '%s\n' 'dpkg-deb is required' >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
root="$work/root"
payload="$work/payload"
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/lib/helixir" "$payload" "$output"
tar -xzf "$archive" -C "$payload"

for required in helixir helixir-mcp helixir-deploy schema/schema.hx schema/queries.hx \
  skills/helixir-memory/SKILL.md helix.toml backend-image.json; do
  [[ -e "$payload/$required" ]] || {
    printf 'release archive is missing %s\n' "$required" >&2
    exit 1
  }
done

cp -a "$payload/." "$root/usr/lib/helixir/"
ln -s ../lib/helixir/helixir "$root/usr/bin/helixir"
ln -s ../lib/helixir/helixir-mcp "$root/usr/bin/helixir-mcp"
ln -s ../lib/helixir/helixir-deploy "$root/usr/bin/helixir-deploy"

installed_size=$(du -sk "$root/usr" | awk '{print $1}')
cat >"$root/DEBIAN/control" <<EOF
Package: helixir
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: Nikita Rulenko <nikita-rulenko@users.noreply.github.com>
Depends: ca-certificates, libc6 (>= 2.35), libssl3
Installed-Size: $installed_size
Homepage: https://github.com/nikita-rulenko/Helixir
Description: graph-based persistent memory for LLM agents
 Helixir provides typed graph memory, hybrid retrieval, reasoning chains,
 permanent graph-backed RBAC, and an MCP server for coding agents.
 Run helixir onboard after package installation to configure the system.
EOF

cat >"$root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
  printf '%s\n' 'Helixir installed. Run: helixir onboard'
fi
exit 0
EOF
chmod 0755 "$root/DEBIAN/postinst"
find "$root/usr/lib/helixir" -type d -exec chmod 0755 {} +
find "$root/usr/lib/helixir" -type f -exec chmod 0644 {} +
chmod 0755 "$root/usr/lib/helixir/helixir" \
  "$root/usr/lib/helixir/helixir-mcp" "$root/usr/lib/helixir/helixir-deploy"
find "$root/usr/lib/helixir" -type f \( -name '*.so' -o -name '*.so.*' \) -exec chmod 0755 {} +

source_date_epoch=${SOURCE_DATE_EPOCH:-0}
[[ "$source_date_epoch" =~ ^[0-9]+$ ]] || {
  printf 'SOURCE_DATE_EPOCH must be an integer, got %s\n' "$source_date_epoch" >&2
  exit 2
}
find "$root" -exec touch -h -d "@$source_date_epoch" {} +
export SOURCE_DATE_EPOCH="$source_date_epoch"

package="$output/helixir_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "$root" "$package"
printf '%s\n' "$package"
