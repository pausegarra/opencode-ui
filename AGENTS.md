# Repository Notes

- `opencode-ui` is a Tauri 2 desktop shell for the local OpenCode web UI.
- Frontend entrypoint: `src/main.js`; styles: `src/styles.css`; Tauri backend: `src-tauri/src/main.rs`; app config: `src-tauri/tauri.conf.json`.
- Generated/build output: `dist/`, `src-tauri/target/`, and `src-tauri/gen/schemas/*`. Do not edit generated files by hand.
- Package manager is npm. CI and release use Node 20 and `npm ci`.

## Common Commands

- `npm install`
- `npm run dev` starts Vite only.
- `npm run tauri:dev` starts the desktop app.
- `npm run build` builds the frontend.
- `npm run tauri:build` builds the full Tauri app.

## Runtime Wiring

- Rust exposes Tauri commands `bootstrap` and `set_binary_path`.
- Frontend listens for `opencode://status` and `opencode://log` events.
- App probes `http://127.0.0.1:4096/` and reuses an existing local OpenCode server when possible.
- If no server is up, it launches `opencode web` and only manages the child process it started.
- Env vars honored: `OPENCODE_BIN` and `OPENCODE_SERVER_PASSWORD`.
- Persistent config lives under `dirs::config_dir()/opencode-ui/config.json`.

## Release Rules

- `.github/workflows/release.yml` only runs on SemVer tags like `1.2.3`.
- Release builds target macOS, Linux, and Windows via `tauri-apps/tauri-action`.

## Verification

- There is no dedicated test or lint script in `package.json`.
- Use `npm run build` for frontend sanity checks and `npm run tauri:build` for full-app verification.
- If you add Rust tests, run them from `src-tauri/` with `cargo test`.
