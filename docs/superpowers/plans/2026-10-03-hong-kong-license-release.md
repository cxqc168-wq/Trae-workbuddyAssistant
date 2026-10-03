# Hong Kong license server and Windows 2.4.8 release

> **For agentic workers:** Use superpowers:executing-plans to implement and verify each step in this session.

**Goal:** Point both license-guard clients at the user's Hong Kong server and publish verified, self-contained MSI and EXE installers to the existing GitHub repository.

**Architecture:** Retain the native Tauri license-guard protocol and existing public key. Inspect the service already running on the new host before changing its deployment; retain activation settings and signing identity. Reuse the complete Python, browser and offline WebView2 packaging pipeline.

**Tech Stack:** Rust/Tauri, Python/FastAPI, systemd, WiX, NSIS, GitHub CLI.

**Spec:** User request in this conversation, 2026-10-03.

## Global constraints

- Endpoint: `http://154.222.24.81:8443`.
- No SSH password, signing private key, activation code or user account data in source or release assets.
- Do not replace existing signing keys or unrelated services.
- Validate both actual installer payloads before publishing.

## Review focus

- SSH host key differs from known_hosts: require independently confirmed identity before authentication.
- New server health is insufficient: verify a signed activation response with the embedded public key.
- Existing license-guard worktree changes: preserve unrelated dialog changes.
- Resource completeness: verify every manifest hash and smoke-test extracted payloads.
- Release selection: upload only the two current-version installers and their verification assets.

## Tasks

- [x] Confirm SSH host identity, inspect existing service, and preserve or migrate matching signing/configuration data.
- [x] Update native and external Python client endpoint, version metadata and current documentation.
- [ ] Build MSI and NSIS EXE; run existing tests and extracted-payload verification.
- [ ] Review changes, commit and push to the user's existing repository.
- [ ] Publish v2.4.8 and compare remote asset sizes and SHA256 with local verified artifacts.

## Execution record

- Initial tree clean; version 2.4.7 already published to the user's repository.
- Both old and new public health endpoints return HTTP 200 with status ok.
- SSH connection stopped before password authentication because the new host key differs from known_hosts. User fingerprint confirmation is pending.
- Ruling: prepare client configuration and build while waiting; publication depends on verifying the new service's signing identity.
- User explicitly confirmed trusting the current ED25519 fingerprint; subsequent connections pin it exactly.
- Hong Kong service was already active with the same public key. No server redeployment or key replacement was needed.
- Actual activation response passed RSA-SHA256 verification, random test machine binding and 7-day validity; no user credential files were modified.
- Fresh validation: 73 Rust tests passed; Python auto-checkin tests passed; both client endpoints and version metadata match.
- External license-guard suite: 38 tests passed. Independent review found no actionable issue.
- Compiled release executable contains the new default endpoint and does not contain the old endpoint.
- Ruling: commit and push the verified source while installer compression runs. Publishing the release remains gated on both extracted-payload checks.
