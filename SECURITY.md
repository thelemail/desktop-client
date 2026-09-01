# Security policy

## Reporting a vulnerability

Send reports to **security@thel.email**. A PGP key is available on request.

Please include enough detail to reproduce the issue: affected version or commit, the steps involved, and what an attacker gains. If you have a proof of concept, include it.

We aim to acknowledge a report within three working days and to keep you updated as we work on a fix. Please give us a reasonable opportunity to release one before disclosing publicly.

Do not test against other people's accounts or mailboxes. Register your own account for testing.

## Scope

This repository is the macOS desktop client. Findings that are in scope here include:

- Anything that causes plaintext, private keys, or the account master key to leave the machine, or to reach the webview when it should not
- Weaknesses in key generation, key derivation, vault wrapping, or the OPAQUE exchange as implemented here
- Any path by which the webview can reach the network, the filesystem, or the keychain other than through the commands the Rust core exposes, or can invoke a command with an account it should not have access to
- Cross-site scripting or code execution through the rendering of received mail, and any escape from the sandboxed frame it is rendered in
- Reading or tampering with the local mirror without the keychain key, weaknesses in how that key is stored, or mail that is retained after an account is signed out
- Failures in directory key verification or transparency log proof checking that would let a substituted key be accepted
- Any way to make the app install or run code the user did not ask for, including anything that defeats the user-initiated-only update policy
- A release whose signature, notarization, provenance or checksums do not match the commit it claims

Findings in the shared UI belong to [thelemail/web-client](https://github.com/thelemail/web-client), and server-side issues to the backend, but report them to the same address and we will route them.

## Out of scope

- Reports that a particular hardening flag or entitlement is absent, without a working proof of concept that its absence is exploitable here
- Automated scanner output without a working proof of concept
- Denial of service through volume alone
- Social engineering, and physical attacks
- Attacks that require an already-compromised macOS account, since the local mirror key is held by the keychain of that account
- Vulnerabilities in dependencies with no demonstrated path to exploitation here
