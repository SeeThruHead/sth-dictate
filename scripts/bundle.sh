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

keychain="$HOME/Library/Keychains/sth-dictate-signing.keychain-db"
pass_file="$HOME/.config/sth-dictate/keychain-pass"
identity="SthDictate Local"

if [[ -f "$keychain" && -f "$pass_file" ]]; then
  security unlock-keychain -p "$(cat "$pass_file")" "$keychain"
  codesign -s "$identity" --keychain "$keychain" --force build/SthDictate.app
  echo "signed with $identity, permissions survive rebuilds"
else
  codesign -s - --force build/SthDictate.app
  echo "signed ad hoc, macOS will ask for permissions again after each rebuild"
fi
echo "built build/SthDictate.app ($version)"
