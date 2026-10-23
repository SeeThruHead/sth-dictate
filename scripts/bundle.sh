#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:-1.0}"
swift build -c release --product SthDictate
app="build/SthDictate.app/Contents"
rm -rf build/SthDictate.app
mkdir -p "$app/MacOS"
cp .build/release/SthDictate "$app/MacOS/SthDictate"
sed "s#<string>1.0</string>#<string>$version</string>#g" Resources/Info.plist > "$app/Info.plist"
codesign -s - --force build/SthDictate.app
echo "built build/SthDictate.app ($version)"
