#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <version>   (e.g. $0 0.1.4)" >&2
  exit 1
fi

VERSION="$1"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be N.N.N (got: $VERSION)" >&2
  exit 1
fi

cd "$(dirname "$0")/.."

sed -i.bak "s/^version = \".*\"/version = \"${VERSION}\"/" src-tauri/Cargo.toml
sed -i.bak "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json
rm src-tauri/Cargo.toml.bak src-tauri/tauri.conf.json.bak

echo "Bumped to ${VERSION}:"
grep "^version" src-tauri/Cargo.toml
grep '"version"' src-tauri/tauri.conf.json

git add src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "Release v${VERSION}"

echo
echo "Next: git push, then dispatch the Build Binaries workflow with tag v${VERSION}"
