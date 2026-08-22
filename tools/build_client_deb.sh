#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' 'usage: build_client_deb.sh --archive FILE --version VERSION --arch amd64|arm64 --output DIR'
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
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/lib/helixir-client" \
  "$root/usr/share/helixir-client" "$payload" "$output"
tar -xzf "$archive" -C "$payload"

# Release archives assembled on macOS may contain AppleDouble sidecars even
# when the source tree itself is clean. They are Finder metadata, not Helixir
# payload, and must never be installed under /usr/share.
find "$payload" -type f \( -name '._*' -o -name '.DS_Store' \) -delete

for required in helixir-client skills/helixir-memory/SKILL.md \
  integration/AGENTS.md integration/SKILLS.md; do
  [[ -e "$payload/$required" ]] || {
    printf 'release archive is missing %s\n' "$required" >&2
    exit 1
  }
done

install -m 0755 "$payload/helixir-client" "$root/usr/lib/helixir-client/helixir-client"
cp -a "$payload/skills" "$root/usr/share/helixir-client/"
cp -a "$payload/integration" "$root/usr/share/helixir-client/"
ln -s ../lib/helixir-client/helixir-client "$root/usr/bin/helixir-client"

installed_size=$(du -sk "$root/usr" | awk '{print $1}')
cat >"$root/DEBIAN/control" <<EOF
Package: helixir-client
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: Nikita Rulenko <nikita-rulenko@users.noreply.github.com>
Depends: ca-certificates, libc6 (>= 2.35)
Installed-Size: $installed_size
Homepage: https://github.com/nikita-rulenko/Helixir
Description: thin remote-agent client for a Helixir MCP gateway
 Connect Codex, Claude Code, or Cursor on this host to an existing Helixir
 gateway. The package installs no database, models, daemon, or admin UI.
 Run helixir-client connect after installation.
EOF

cat >"$root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
  printf '%s\n' 'Helixir client installed. Run: helixir-client connect'
fi
exit 0
EOF
chmod 0755 "$root/DEBIAN/postinst"
find "$root/usr" -type d -exec chmod 0755 {} +
find "$root/usr" -type f -exec chmod 0644 {} +
chmod 0755 "$root/usr/lib/helixir-client/helixir-client"

source_date_epoch=${SOURCE_DATE_EPOCH:-0}
[[ "$source_date_epoch" =~ ^[0-9]+$ ]] || {
  printf 'SOURCE_DATE_EPOCH must be an integer, got %s\n' "$source_date_epoch" >&2
  exit 2
}
find "$root" -exec touch -h -d "@$source_date_epoch" {} +
export SOURCE_DATE_EPOCH="$source_date_epoch"

package="$output/helixir-client_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "$root" "$package"
printf '%s\n' "$package"
