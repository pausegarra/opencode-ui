# OpenCode UI

Small Tauri desktop shell for the local OpenCode web UI.

## What It Does

- Checks for an available `opencode` binary.
- Reuses an already running local OpenCode server when possible.
- Starts `opencode web` when the server is not up yet.
- Waits for `http://127.0.0.1:4096/` to answer before switching the window.
- Keeps the child process tied to the app lifecycle.
- Stops only the process started by this app.

## Run On macOS

```bash
npm install
npm run tauri:dev
```

Production build:

```bash
npm run tauri:build
```

## Config

The app stores a small JSON config in the user local data directory:

- `binary_path`
- `port`
- `working_dir`
- `extra_args`
- `server_password`

It also honors:

- `OPENCODE_BIN`
- `OPENCODE_SERVER_PASSWORD`

## Notes

- The main window starts on a local loading screen, then navigates directly to the local OpenCode URL.
- No iframe is used.
- No remote backend is required.
- The launch lock prevents this app from spawning multiple OpenCode children at the same time.
- The error state includes both a text field and a native file picker for the OpenCode binary.

## Known Limits

- First version targets macOS only.
- The app trusts the OpenCode web server on `127.0.0.1:4096`.
- If another service answers on that port with a page that does not look like OpenCode, the app may wait until startup times out.
- If OpenCode changes its root page shape dramatically, the startup probe may need refinement.
