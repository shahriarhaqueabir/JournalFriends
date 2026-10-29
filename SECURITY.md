# Security Policy

## Reporting a Vulnerability

If you discover a security issue in Arnold, please report it privately rather than
opening a public issue. Email the maintainer at the address listed on the GitHub
profile, or open a GitHub Security Advisory on this repository.

Please include:

- A description of the issue and its impact.
- Steps to reproduce (or a proof of concept).
- The affected version / commit.

You will receive an acknowledgement within 7 days. Fixes are released as patch
versions and credited in the changelog unless you prefer to remain anonymous.

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Security Model

Arnold is offline-first by construction:

- **No network for the AI.** There is no URL-fetch, web-search, or
  model-download tool. The only HTTP traffic is loopback to the two
  app-managed `llama-server` processes (`127.0.0.1:8080` / `:8081`).
- **Content-Security-Policy** restricts `connect-src` to `self` and the
  loopback engine ports; no remote origin is reachable from the UI.
- **Confirmation-gated writes.** Every AI mutation of a task, event, note, or
  memory is a proposal stored in `proposed_inbox` with status `pending`; it is
  applied only when the user explicitly accepts it. The AI never writes
  directly.
- **Encrypted at rest.** The SQLite database is opened with SQLCipher
  (`PRAGMA key`), and journal content is additionally AES-GCM encrypted with a
  key derived from the vault password (Argon2). A plaintext legacy database is
  migrated automatically on first unlock.
- **No telemetry.** Nothing leaves the machine.

## Known Limitations

- The Windows installer is not code-signed yet. SmartScreen may warn on first
  run; verify the published SHA-256 before installing.
- The bundled `llama-server` runtime is a third-party binary (llama.cpp,
  MIT-licensed). Its license is shipped alongside it in `engines/`.
- `cargo audit` is run in CI; any advisory that cannot be resolved without a
  breaking change is documented in the changelog.
- Remaining `cargo audit` warnings (all non-vulnerability, allowed): `fxhash`,
  `proc-macro-error`, `unic-*`, `zbus_names`/`zvariant*` (unmaintained
  transitive deps) and `chacha20` (yanked). `glib`'s unsoundness advisory
  (RUSTSEC-2024-0429) applies only to the Linux/GTK code path and is not
  reachable on Windows, where Tauri uses WebView2. `tokio`'s I/O-safety
  advisory (RUSTSEC-2026-0386) does not affect this codebase (no `AsyncFd`
  usage).
