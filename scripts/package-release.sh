#!/usr/bin/env bash

set -euo pipefail

release_tag="${1:?Usage: package-release.sh TAG TARGET}"
release_target="${2:?Usage: package-release.sh TAG TARGET}"
workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$workspace_root"

if [[ ! "$release_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$ ]]; then
  echo "Invalid release tag: $release_tag" >&2
  exit 1
fi
case "$release_target" in
  x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;;
  *) echo "Unsupported release target: $release_target" >&2; exit 1 ;;
esac

package_name="trees-${release_tag}-${release_target}"
package_dir="$(mktemp -d "${TMPDIR:-/tmp}/trees-release.XXXXXX")"
trap 'rm -rf "$package_dir"' EXIT
mkdir -p "$package_dir/$package_name" target/dist
binary="target/$release_target/release/trees"
test "$("$binary" -V)" = "trees ${release_tag#v}"
"$binary" --version > "$package_dir/$package_name/BUILD_INFO.txt"
test -s target/THIRD_PARTY_NOTICES.html
cp "$binary" LICENSE target/THIRD_PARTY_NOTICES.html "$package_dir/$package_name/"
tar -czf "target/dist/$package_name.tar.gz" -C "$package_dir" "$package_name"
