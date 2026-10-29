#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

require() {
	if [[ -z "${!1:-}" ]]; then
		echo "release-windows: $1 is not set" >&2
		exit 1
	fi
}

require TAURI_SIGNING_PRIVATE_KEY
require TAURI_SIGNING_PRIVATE_KEY_PASSWORD
signing_key="$TAURI_SIGNING_PRIVATE_KEY"
signing_password="$TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

version="$(python -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
installer="target/release/bundle/nsis/Thelemail_${version}_x64-setup.exe"

python -c "import json,sys;sys.exit(0 if json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'].strip() else 1)" \
	|| { echo "release-windows: plugins.updater.pubkey is empty in tauri.conf.json" >&2; exit 1; }

THELEMAIL_RELEASE=1 node scripts/build-frontend.mjs
npx --yes "@tauri-apps/cli@${TAURI_CLI_VERSION:-2.12.4}" build

[[ -f "$installer" ]] || { echo "release-windows: no installer at $installer" >&2; exit 1; }

rm -f "$installer.sig"
TAURI_SIGNING_PRIVATE_KEY="$signing_key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$signing_password" \
	npx --yes "@tauri-apps/cli@${TAURI_CLI_VERSION:-2.12.4}" signer sign "$installer"
cargo run --quiet --release -p thelemail-release --bin verify-update -- \
	src-tauri/tauri.conf.json "$installer" "$installer.sig"

sha256sum "$installer" | tee "$installer.sha256"
echo "release-windows: $installer"
