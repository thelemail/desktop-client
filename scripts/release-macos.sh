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
require TAURI_SIGNING_PRIVATE_KEY
require TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export APPLE_TEAM_ID
signing_key="$TAURI_SIGNING_PRIVATE_KEY"
signing_password="$TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

app="target/release/bundle/macos/Thelemail.app"
version="$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
dmg="target/release/Thelemail_${version}_aarch64.dmg"
updater="target/release/Thelemail_${version}_aarch64.app.tar.gz"
app_zip="target/release/Thelemail_${version}_aarch64.app.zip"

python3 -c "import json,sys;sys.exit(0 if json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'].strip() else 1)" \
	|| { echo "release-macos: plugins.updater.pubkey is empty in tauri.conf.json" >&2; exit 1; }

[[ -f "$APPLE_API_KEY_PATH" ]] || { echo "release-macos: no key at $APPLE_API_KEY_PATH" >&2; exit 1; }

THELEMAIL_RELEASE=1 node scripts/build-frontend.mjs
npx --yes "@tauri-apps/cli@${TAURI_CLI_VERSION:-2.11.4}" build

codesign --force --options runtime --timestamp \
	--sign "$APPLE_SIGNING_IDENTITY" \
	"$app"

codesign --verify --strict --verbose=2 "$app"

rm -f "$app_zip"
ditto -c -k --keepParent "$app" "$app_zip"
xcrun notarytool submit "$app_zip" \
	--key "$APPLE_API_KEY_PATH" \
	--key-id "$APPLE_API_KEY_ID" \
	--issuer "$APPLE_API_ISSUER" \
	--wait
rm -f "$app_zip"
xcrun stapler staple "$app"
spctl --assess --type execute -vv "$app"

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

rm -f "$updater" "$updater.sig"
COPYFILE_DISABLE=1 tar -C "$(dirname "$app")" -czf "$updater" "$(basename "$app")"
TAURI_SIGNING_PRIVATE_KEY="$signing_key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$signing_password" \
	npx --yes "@tauri-apps/cli@${TAURI_CLI_VERSION:-2.11.4}" signer sign "$updater"
cargo run --quiet --release -p thelemail-release --bin verify-update -- \
	src-tauri/tauri.conf.json "$updater" "$updater.sig"

shasum -a 256 "$dmg" | tee "$dmg.sha256"
echo "release-macos: $dmg"
echo "release-macos: $updater"
