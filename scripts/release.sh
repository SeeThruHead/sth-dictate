#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: release.sh <version>}"
./scripts/bundle.sh "$version"
zip="build/SthDictate-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent build/SthDictate.app "$zip"
echo "$zip"
shasum -a 256 "$zip" | cut -d' ' -f1
