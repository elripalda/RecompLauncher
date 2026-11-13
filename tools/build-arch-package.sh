#!/usr/bin/env bash
# Build the current checkout, including local edits, without copying build caches into the sources.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
out="$root/dist/arch"
mkdir -p "$out"
cd "$root"
version=$(sed -n 's/^  "version": "\([^"]*\)",$/\1/p' src-tauri/tauri.conf.json)
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z]+\.[0-9]+)?$ ]] || {
  echo "Unsupported package version: $version" >&2
  exit 1
}
# A separator before 'beta' sorts after the stable version in pacman; omit it instead.
pkgver=${version/-/}
git ls-files -z --cached --others --exclude-standard -- src-tauri ui docs LICENSE package.json \
  | tar --null --verbatim-files-from --files-from=- --transform='s,^,recomplauncher/,' -czf "$out/recomplauncher-source.tar.gz"
digest=$(sha256sum "$out/recomplauncher-source.tar.gz")
sed -e "s/@PKGVER@/$pkgver/" -e "s/@SHA256@/${digest%% *}/" packaging/arch/PKGBUILD.in > "$out/PKGBUILD"
cd "$out"
exec makepkg "$@"
