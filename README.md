# Thelemail desktop client

The macOS client for [Thelemail](https://thelemail.com), an end-to-end encrypted email service. It is a [Tauri 2](https://tauri.app) shell around the [web client](https://github.com/thelemail/web-client) UI, with a Rust core that owns the private keys, an encrypted local mirror of the mailbox, and a background sync loop.

The webview never holds a private key. Every cryptographic operation — OPAQUE login, key generation, message decryption, signing, attachment framing — happens in Rust, and the webview receives plaintext results over Tauri's IPC. The UI is packaged into the binary and served from a local scheme; the app never loads the web client over the network as executable content.

## Architecture

| Path | Contents |
| --- | --- |
| `src-tauri/` | The Tauri application: commands, tray, windowing, network and keychain access |
| `crates/thelemail-crypto/` | OpenPGP, OPAQUE, the account master key schedule, attachment framing |
| `crates/thelemail-keystore/` | Unlocked vaults, alias keys, and the operations the UI is allowed to ask for |
| `crates/thelemail-store/` | The encrypted per-account mirror and its full-text index |
| `crates/thelemail-mime/` | MIME extraction and PGP/MIME assembly |
| `crates/thelemail-api/` | HTTP transport, restricted to the configured API, submission and object storage origins |
| `overlay/` | The desktop implementation of the web client's `$platform` seam |

The web client is written against a small platform interface rather than the browser directly. `overlay/` supplies the desktop implementation of it, so every request, keystore call, blob fetch and file save is routed through Rust instead of `fetch` and the DOM. Nothing in the UI reaches the network on its own.

Mail is mirrored into a per-account SQLCipher database with a SQLite FTS5 index, so search and reading work offline and never leave the machine. The database key is a random 256-bit value held in the macOS keychain, bound to the device where the signed build's entitlements allow it. How much mail to keep is asked on first run and can be changed later.

## Cryptography

| Crate | Used for |
| --- | --- |
| `pgp` | OpenPGP message encryption, signing and key handling |
| `opaque-ke` | OPAQUE password-authenticated key exchange, so the password never reaches the server |
| `argon2` | Key stretching inside OPAQUE |
| `hkdf`, `sha2`, `aes-gcm` | The account master key schedule and vault wrapping |
| `bip39` | Recovery phrase generation |
| `rusqlite` (SQLCipher) | Encryption of the local mirror at rest |
| `security-framework` | Keychain storage of the mirror key |

The wire formats are byte-compatible with the browser client: an account can be used from either, and the interop tests in `crates/thelemail-crypto/tests/` prove it in both directions against `openpgp.js` and `@serenity-kit/opaque`.

## Development

Requires Rust and Node. The UI comes from the `web-client` submodule, pinned to the version in `web-client.version`.

```bash
git submodule update --init
node scripts/build-frontend.mjs
cargo run -p thelemail-desktop --features devtools
```

To build the UI from a working copy instead of the pin:

```bash
THELEMAIL_WEB_CLIENT_DIR=../web-client node scripts/build-frontend.mjs
```

The override prints a warning on every build and is rejected during a release. Its `.env` supplies the origins, so it is the way to run against a local backend.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

## Configuration

A release build needs no configuration: the API, submission and object storage origins are compiled in, and the trust roots come from the pinned web client. For development the following override them at runtime.

| Variable | Meaning |
| --- | --- |
| `THELEMAIL_DESKTOP_API_BASE_URL` | Base URL of the Thelemail API |
| `THELEMAIL_DESKTOP_SUBMISSION_BASE_URL` | Base URL of the mail submission service |
| `THELEMAIL_DESKTOP_BLOB_ORIGIN` | Origin of the object storage the API issues presigned URLs for |
| `THELEMAIL_DESKTOP_WEB_ORIGIN` | Origin the app hands off to for billing, and the `Origin` it presents to the API |

Requests to any other host are refused by the transport rather than merely discouraged.

## Updates

The app never installs anything on its own. About a minute after launch and then every six hours it fetches `latest.json` from the latest GitHub release, and when a newer version is listed it shows a banner in the mail view and an entry under Settings, About & updates. Nothing is downloaded until the person using the app chooses Install. Later hides that version for three days; a newer release shows up straight away.

Choosing Install downloads `Thelemail_<version>_aarch64.app.tar.gz` and accepts it only if all of these hold:

1. The archive carries a minisign signature from the release key whose public half is pinned in `src-tauri/tauri.conf.json`.
2. The unpacked bundle is `com.thelemail.desktop`, its version equals the one the manifest announced, and that version is newer than the running one. An older signed build relabelled as new is refused, and so is any downgrade.
3. `codesign` accepts the bundle against a Developer ID requirement pinned to our Apple team, and Gatekeeper accepts its notarization.

The bundle is unpacked next to the running app and verified there, so a failed check leaves the installed app untouched. Before the swap the app refuses to restart while a message is sending, an attachment is uploading or a draft cannot be saved. It then stops sync, closes every local database and exchanges the two bundles in one atomic rename. If the app runs from a translocated or read-only location it never asks for administrator rights; it points to the release page instead.

The update channel is not TLS-pinned, because GitHub rotates its certificates. The trust comes from the two signatures above, so whoever controls the network or the release host still cannot get code installed.

## Releases

A release is built from a `v*` tag, signed with a Developer ID certificate and notarized by Apple. The build is reproducible in the sense that matters: the tag records the exact web client version the UI was built from, and `release-metadata.json` in each release names both commits.

Every release carries `SHA256SUMS`, a Sigstore signature over it, an SBOM, and build provenance:

```bash
shasum -a 256 -c SHA256SUMS

cosign verify-blob --bundle SHA256SUMS.sigstore.json \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github\.com/thelemail/desktop-client/\.github/workflows/release\.yml@refs/tags/v.+$' \
  SHA256SUMS

gh attestation verify Thelemail_*.dmg \
  --repo thelemail/desktop-client \
  --signer-workflow thelemail/desktop-client/.github/workflows/release.yml
```

And that macOS agrees the build is signed and notarized:

```bash
spctl --assess --type open --context context:primary-signature -v Thelemail_*.dmg
```

## Security

See [SECURITY.md](SECURITY.md) for how to report a vulnerability.

## Licence

[GNU AGPL v3](LICENSE).
