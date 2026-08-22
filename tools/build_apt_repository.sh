#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' 'usage: build_apt_repository.sh --packages DIR --output DIR --signing-key KEY_ID'
}

packages=''
output=''
signing_key=''
while (($#)); do
  case "$1" in
    --packages) packages=${2:?}; shift 2 ;;
    --output) output=${2:?}; shift 2 ;;
    --signing-key) signing_key=${2:?}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ -d "$packages" && -n "$output" && -n "$signing_key" ]] || { usage >&2; exit 2; }
for command in apt-ftparchive dpkg-scanpackages dpkg-deb gpg; do
  command -v "$command" >/dev/null || { printf '%s is required\n' "$command" >&2; exit 1; }
done

if [[ -e "$output" ]] && [[ -n "$(find "$output" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf 'output directory must be absent or empty: %s\n' "$output" >&2
  exit 2
fi
pool="$output/pool/main/h/helixir"
mkdir -p "$pool" "$output/dists/stable/main/binary-amd64" \
  "$output/dists/stable/main/binary-arm64"
find "$packages" -maxdepth 1 -type f \
  \( -name 'helixir_[0-9]*.deb' -o -name 'helixir-client_[0-9]*.deb' \) \
  -exec cp -p {} "$pool/" \;
for package in helixir helixir-client; do
  [[ -n "$(find "$pool" -maxdepth 1 -type f -name "${package}_[0-9]*.deb" -print -quit)" ]] || {
    printf 'missing %s Debian packages in %s\n' "$package" "$packages" >&2
    exit 2
  }
done

for arch in amd64 arm64; do
  index="$output/dists/stable/main/binary-$arch/Packages"
  # Keep every published version in the index. This makes explicit version
  # pinning and rollback-to-an-older-package possible without changing the
  # repository URL; apt still selects the newest version by default.
  (cd "$output" && dpkg-scanpackages --multiversion -a "$arch" pool /dev/null) >"$index"
  grep -qx 'Package: helixir' "$index"
  grep -qx 'Package: helixir-client' "$index"
  gzip -9 -n -c "$index" >"$index.gz"
done

release="$output/dists/stable/Release"
(
  cd "$output/dists/stable"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Helixir \
    -o APT::FTPArchive::Release::Label=Helixir \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures='amd64 arm64' \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Description='Helixir signed package repository' \
    release .
) >"$release"

gpg --batch --yes --local-user "$signing_key" --armor --detach-sign \
  --output "$release.gpg" "$release"
gpg --batch --yes --local-user "$signing_key" --clearsign \
  --output "$output/dists/stable/InRelease" "$release"
gpg --batch --yes --local-user "$signing_key" --export \
  --output "$output/helixir-archive-keyring.gpg"
gpg --batch --with-colons --fingerprint "$signing_key" \
  | awk -F: '$1 == "fpr" {print $10; exit}' >"$output/KEY-FINGERPRINT"

touch "$output/.nojekyll"
printf '%s\n' "$output"
