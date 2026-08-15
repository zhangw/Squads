# Security Hardening

This file documents the remediation of the codex-security scan findings
(2026-08) for the desktop client and the REST server.

## Desktop client (src/)

- **Skype-token exfiltration via message images (high)**: AMS image URLs are
  validated before any token-bearing fetch. Only HTTPS URLs on Microsoft-owned
  hosts (skype.com / teams.microsoft.com / office.net subdomains) with no
  userinfo or port receive the Skype credential; private/loopback destinations
  are rejected after address resolution (src/security.rs, src/api.rs,
  src/parsing.rs, src/main.rs).
- **Blind Giphy SSRF (medium)**: GIF downloads require allowlisted
  `giphy.com` origins, redirects are disabled, resolved private/loopback/
  link-local addresses are rejected before the request.
- **Unbounded media downloads (medium)**: every automatic media fetch has a
  20 MiB byte cap, a 30s timeout, MIME magic-byte validation and atomic
  cache writes; the image cache keeps a 256 MiB budget with oldest-first
  eviction (src/security.rs, src/components/cached_image.rs).
- **Remote-content panics (medium)**: image src/width/height attributes and
  Media_Card base64/UTF-8 are parsed defensively with placeholder fallbacks;
  the GIF widget tolerates malformed, empty and single-frame GIFs (frozen
  blank frame, no modulo-by-zero) and caps decoded frames at 600
  (src/parsing.rs, src/widgets/gif.rs).
- **Unsafe link schemes (low)**: only http/https links are passed to
  `webbrowser::open`; file:, data:, javascript: and custom schemes are
  blocked (src/main.rs, src/security.rs).
- **Credential persistence (low)**: `access_tokens.json` is written via a
  create-new 0600 temp file with fsync and atomic rename; symlinks at the
  target are replaced, not followed (src/utils.rs).

## REST server (squads-server/)

- **Suffix contact matching (medium)**: contact ids must be a complete MRI
  (`8:orgid:<id>`) or a bare AAD object id. Partial and suffix forms are
  rejected; authorization and 1:1 chat selection both use the same canonical
  exact identity; sending to yourself is refused.
- **Mutable allowlist names (medium)**: the send allowlist is resolved once
  at startup to immutable thread ids. `SQUADS_ALLOWED_GROUPS` accepts either
  thread ids (`19:...`) or exact display names; names that match zero or
  multiple chats fail closed.
- **Token store (low)**: refresh-token persistence uses a create-new 0600
  temp file, fsync, atomic rename and directory sync (squads-server/src/auth.rs).

## Tests

- Desktop: `cargo test --bin Squads` — 6 security tests (URL allowlists,
  private addresses, link schemes, media magic bytes).
- Server: `cargo test` in squads-server/ — 9 tests including exact contact
  matching, allowlist resolution and fail-closed ambiguity.
