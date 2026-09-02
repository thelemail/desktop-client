#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

require() {
	if [[ -z "${!1:-}" ]]; then
		echo "release-macos: $1 is not set — source the signing env from the deploy repo" >&2
		exit 1
	fi
}

require APPLE_SIGNING_IDENTITY
require APPLE_TEAM_ID
require APPLE_API_KEY_PATH
require APPLE_API_KEY_ID
require APPLE_API_ISSUER

app="target/release/bundle/macos/Thelemail.app"
version="$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
dmg="target/release/Thelemail_${version}_aarch64.dmg"

[[ -f "$APPLE_API_KEY_PATH" ]] || { echo "release-macos: no key at $APPLE_API_KEY_PATH" >&2; exit 1; }

THELEMAIL_RELEASE=1 node scripts/build-frontend.mjs
npx --yes "@tauri-apps/cli@${TAURI_CLI_VERSION:-2.11.4}" build

codesign --force --options runtime --timestamp \
	--sign "$APPLE_SIGNING_IDENTITY" \
	"$app"

codesign --verify --strict --verbose=2 "$app"

staging="$(mktemp -d)"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
rm -f "$dmg"
hdiutil create -volname Thelemail -srcfolder "$staging" -ov -format UDZO "$dmg"
rm -rf "$staging"

codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"

xcrun notarytool submit "$dmg" \
	--key "$APPLE_API_KEY_PATH" \
	--key-id "$APPLE_API_KEY_ID" \
	--issuer "$APPLE_API_ISSUER" \
	--wait

xcrun stapler staple "$dmg"
spctl --assess --type open --context context:primary-signature -vv "$dmg"

shasum -a 256 "$dmg" | tee "$dmg.sha256"
echo "release-macos: $dmg"
