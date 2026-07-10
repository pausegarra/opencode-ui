# Tauri Release + Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build GitHub Releases pipeline for Apple Silicon macOS, Windows, and Linux, plus Tauri auto-updates on app start.

**Architecture:** One GitHub Actions workflow publishes signed release artifacts only from SemVer tags. Tauri updater is enabled in Rust, wired to GitHub Releases `latest.json`, and auto-checks on startup so the app updates itself before normal use.

**Tech Stack:** GitHub Actions, `tauri-action@v0`, Tauri 2, `tauri-plugin-updater`, Rust, Vite/npm.

## Global Constraints

- Release trigger only on SemVer tags matching `*.*.*`.
- macOS build target must be Apple Silicon only (`aarch64-apple-darwin`).
- Linux and Windows release artifacts must still be produced.
- Updater must use GitHub Releases for distribution.
- Auto-check updates on app startup.

---

### Task 1: Add release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: repository tag `github.ref_name`, GitHub token, Tauri signing secret.
- Produces: GitHub Release with signed Tauri artifacts and updater metadata.

- [ ] **Step 1: Write workflow**

```yaml
name: Release

on:
  push:
    tags:
      - "*.*.*"

permissions:
  contents: write

jobs:
  validate-tag:
    runs-on: ubuntu-latest
    steps:
      - name: Reject non SemVer tags
        shell: bash
        run: |
          TAG="${GITHUB_REF_NAME}"
          if [[ ! "${TAG}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            echo "::error::Tag '${TAG}' does not match MAJOR.MINOR.PATCH"
            exit 1
          fi

  build-and-release:
    needs: [validate-tag]
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: macos-14
            target: aarch64-apple-darwin
          - platform: ubuntu-22.04
          - platform: windows-latest

    runs-on: ${{ matrix.platform }}

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: "20"

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install Linux system dependencies
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libgtk-3-dev \
            libayatana-appindicator3-dev \
            librsvg2-dev \
            patchelf

      - name: Install UI dependencies
        run: npm ci

      - name: Build and upload Tauri release
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: "${{ github.ref_name }}"
          releaseBody: "Automated release for ${{ github.ref_name }}"
          releaseDraft: false
          prerelease: false
          updaterJsonPreferNsis: true
          args: ${{ matrix.target && '--target ' || '' }}${{ matrix.target || '' }}
```

- [ ] **Step 2: Verify tag rule and matrix**

Run: `gh workflow view Release` after pushing workflow or inspect file locally.
Expected: only `*.*.*` tags trigger; matrix includes `macos-14`, `ubuntu-22.04`, `windows-latest`.

### Task 2: Enable Tauri updater

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: updater public key, GitHub Releases `latest.json`, `tauri-plugin-updater`.
- Produces: startup update check that downloads, installs, and restarts on new versions.

- [ ] **Step 1: Add dependency and config**

```toml
# Cargo.toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-updater = "2"
```

```json
{
  "bundle": {
    "active": true,
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDU2MkE4NjYyM0EyNTVGOApSV1Q0VmFJalpxaGlCZkxmTzhYSlliTXRubWUvVTFNTlB4MTByMFprMHNoK09LRWkwRjVrSi8xVAo=",
      "endpoints": [
        "https://github.com/pausegarra/opencode-ui/releases/latest/download/latest.json"
      ]
    }
  }
}
```

```json
{
  "permissions": ["core:default", "dialog:default", "updater:default"]
}
```

- [ ] **Step 2: Wire updater into startup**

```rust
use tauri_plugin_updater::UpdaterExt;

fn main() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .setup(|app| {
      let handle = app.handle().clone();
      tauri::async_runtime::spawn(async move {
        if let Ok(Some(update)) = handle.updater().check().await {
          let _ = update
            .download_and_install(|_, _| {}, || {})
            .await;
          handle.restart();
        }
      });
      Ok(())
    })
```

- [ ] **Step 3: Verify config loads and updater permission exists**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: build passes with updater dependency and permissions in place.

### Task 3: Verify release behavior

**Files:**
- No new files.

**Interfaces:**
- Consumes: GitHub tag push, workflow file, Tauri build outputs.
- Produces: validated release path for all 3 desktop targets.

- [ ] **Step 1: Dry-check local Rust config**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: no compile errors from updater integration.

- [ ] **Step 2: Verify repo status**

Run: `git status --short`
Expected: only intended workflow/config source changes tracked.
