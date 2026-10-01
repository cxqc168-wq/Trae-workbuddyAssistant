# Self-contained Windows Release Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement and verify each task.

**Goal:** Publish the current project to its GitHub repository and release MSI and EXE installers that include their runtime dependencies.

**Architecture:** Keep the existing native Rust integration of the license-guard protocol and matching public key. Stage a complete official Windows Python runtime, pinned Python packages and Chrome for Testing as Tauri resources; bundle the WebView2 offline installer. Build and inspect both installers before publishing version 2.4.7.

**Tech Stack:** Tauri 2, Rust, React, Windows Python 3.13, WiX, NSIS, GitHub CLI.

**Spec:** User request in this conversation: submit this project to GitHub, use `D:\My_Codeproject\license-guard`, package all libraries, and upload MSI/EXE to Release.

## Global Constraints

- Keep the activation requirement in release builds; include only the authorization public key.
- Exclude account data, certificates, licenses and generated binary resources from source control.
- Installation must not require Node.js, Rust, Python, pip or a separately installed browser.
- Use the existing repository `cxqc168-wq/Trae-workbuddyAssistant` and fast-forward publication.

## Review Focus

- Missing runtime resources must fail the build verification.
- Python must load dependencies from bundled files even when host Python is unavailable.
- Browser resources must include DLLs, locales and data files and launch with an isolated profile.
- Both installer payloads must contain the same verified runtime and offline WebView2 installer.
- Activation must remain enabled in the release executable.

### Task 1: Prepare and verify runtime resources

**Files:** `scripts/prepare-release.py`, `scripts/verify-release-runtime.py`, `src-python/requirements-release.txt`, `package.json`, `.gitignore`.

- [x] Run verification against the existing script-only layout and confirm missing Python is rejected.
- [x] Download official Python with its published SHA256, install pinned Windows wheels, copy application scripts and download a complete pinned browser.
- [x] Verify SSL, SQLite, cryptography, Brotli, DPAPI, proxy CA generation and browser startup without host dependencies.

### Task 2: Build complete installers

**Files:** `src-tauri/tauri.conf.json`, `.github/workflows/release-windows.yml`, version files, release documentation.

- [x] Require resource preparation and verification before production builds; enable offline WebView2 installation.
- [x] Run Rust and Python tests and build both MSI and NSIS EXE.
- [x] Extract installer payloads and repeat runtime checks; generate SHA256 checksums and component manifest.

### Task 3: Publish verified release

- [x] Review source and staged files for secrets and generated account/runtime data.
- [ ] Commit source changes, fast-forward push to the authorized repository and create version 2.4.7.
- [ ] Upload both installers, manifest and checksums; verify remote assets and return the release URL.
