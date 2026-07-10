use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
  env,
  fs,
  io,
  path::{Path, PathBuf},
  sync::Arc,
  time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_updater::UpdaterExt;
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  process::{Child, Command},
  sync::Mutex,
  time::sleep,
};

const DEFAULT_PORT: u16 = 4096;
const DEFAULT_HOST: &str = "127.0.0.1";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const MONITOR_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct AppState {
  inner: Arc<Mutex<State>>,
  client: reqwest::Client,
}

struct State {
  config: AppConfig,
  process: Option<ManagedProcess>,
  booting: bool,
}

struct ManagedProcess {
  child: Child,
  _lock_file: fs::File,
  started_at: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
  #[serde(default = "default_port")]
  port: u16,
  #[serde(default)]
  binary_path: Option<PathBuf>,
  #[serde(default)]
  working_dir: Option<PathBuf>,
  #[serde(default)]
  extra_args: Vec<String>,
  #[serde(default)]
  server_password: Option<String>,
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      port: DEFAULT_PORT,
      binary_path: None,
      working_dir: None,
      extra_args: Vec::new(),
      server_password: env::var("OPENCODE_SERVER_PASSWORD").ok(),
    }
  }
}

#[derive(Debug, Serialize)]
struct BootstrapResponse {
  url: String,
  source: String,
}

#[derive(Debug, Clone, Serialize)]
struct StatusEvent {
  phase: String,
  message: String,
  detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct LogEvent {
  stream: String,
  line: String,
}

#[derive(Debug, thiserror::Error)]
enum BootError {
  #[error("{0}")]
  Message(String),
}

#[tauri::command]
async fn bootstrap(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<BootstrapResponse, String> {
  ensure_ready(app, clone_state(&state)).await.map_err(|err| err.to_string())
}

#[tauri::command]
async fn set_binary_path(path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
  let trimmed = path.trim();
  let mut inner = state.inner.lock().await;
  if trimmed.is_empty() {
    inner.config.binary_path = None;
  } else {
    inner.config.binary_path = Some(PathBuf::from(trimmed));
  }
  persist_config(&inner.config).map_err(|err| err.to_string())?;
  Ok(())
}

fn default_port() -> u16 {
  DEFAULT_PORT
}

fn config_file_path() -> Option<PathBuf> {
  let base = dirs::config_dir()?;
  Some(base.join("opencode-ui").join("config.json"))
}

fn load_config() -> AppConfig {
  let Some(path) = config_file_path() else {
    return AppConfig::default();
  };

  let mut config = match fs::read_to_string(&path) {
    Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
    Err(_) => AppConfig::default(),
  };

  if config.server_password.is_none() {
    config.server_password = env::var("OPENCODE_SERVER_PASSWORD").ok();
  }

  config
}

fn clone_state(state: &tauri::State<'_, AppState>) -> AppState {
  AppState {
    inner: state.inner.clone(),
    client: state.client.clone(),
  }
}

fn persist_config(config: &AppConfig) -> io::Result<()> {
  let Some(path) = config_file_path() else {
    return Ok(());
  };

  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }

  let data = serde_json::to_vec_pretty(config).unwrap_or_default();
  fs::write(path, data)
}

fn emit_status(app: &AppHandle, phase: &str, message: &str, detail: Option<String>) {
  let _ = app.emit(
    "opencode://status",
    StatusEvent {
      phase: phase.to_string(),
      message: message.to_string(),
      detail,
    },
  );
}

fn emit_log(app: &AppHandle, stream: &str, line: String) {
  println!("[{stream}] {line}");
  let _ = app.emit(
    "opencode://log",
    LogEvent {
      stream: stream.to_string(),
      line,
    },
  );
}

fn local_url(port: u16) -> String {
  format!("http://{DEFAULT_HOST}:{port}/")
}

fn candidate_paths() -> Vec<PathBuf> {
  let mut paths = Vec::new();

  if let Some(path) = env::var_os("OPENCODE_BIN").map(PathBuf::from) {
    paths.push(path);
  }

  if let Some(path_var) = env::var_os("PATH") {
    for entry in env::split_paths(&path_var) {
      paths.push(entry.join("opencode"));
    }
  }

  if let Some(home) = dirs::home_dir() {
    paths.push(home.join(".cargo/bin/opencode"));
    paths.push(home.join(".local/bin/opencode"));
  }

  paths.push(PathBuf::from("/opt/homebrew/bin/opencode"));
  paths.push(PathBuf::from("/usr/local/bin/opencode"));
  paths.push(PathBuf::from("/usr/bin/opencode"));
  paths.push(PathBuf::from("/Applications/OpenCode.app/Contents/MacOS/opencode"));

  paths
}

fn executable_exists(path: &Path) -> bool {
  if !path.exists() {
    return false;
  }

  let Ok(meta) = fs::metadata(path) else {
    return false;
  };

  if !meta.is_file() {
    return false;
  }

  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
  }

  #[cfg(not(unix))]
  {
    true
  }
}

fn resolve_binary(config: &AppConfig) -> Result<PathBuf, BootError> {
  if let Some(path) = &config.binary_path {
    let candidate = if path.is_absolute() {
      path.clone()
    } else {
      env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.clone())
    };

    if executable_exists(&candidate) {
      return Ok(candidate);
    }

    return Err(BootError::Message(format!(
      "Custom OpenCode path is not executable: {}",
      candidate.display()
    )));
  }

  for candidate in candidate_paths() {
    if executable_exists(&candidate) {
      return Ok(candidate);
    }
  }

  Err(BootError::Message(
    "OpenCode binary not found. Set OPENCODE_BIN or use a custom path.".to_string(),
  ))
}

async fn probe_server(client: &reqwest::Client, url: &str) -> Result<bool, reqwest::Error> {
  let response = client.get(url).send().await?;
  if !response.status().is_success() {
    return Ok(false);
  }

  let body = response.text().await.unwrap_or_default();
  let lower = body.to_lowercase();
  Ok(lower.contains("opencode"))
}

async fn ensure_ready(app: AppHandle, state: AppState) -> Result<BootstrapResponse, BootError> {
  let url = {
    let inner = state.inner.lock().await;
    local_url(inner.config.port)
  };

  emit_status(
    &app,
    "checking",
    "Checking OpenCode",
    Some("Probing the local server first.".to_string()),
  );

  if probe_server(&state.client, &url).await.unwrap_or(false) {
    emit_status(
      &app,
      "ready",
      "OpenCode is already running",
      Some("Reusing the existing local instance.".to_string()),
    );
    return Ok(BootstrapResponse {
      url,
      source: "existing".to_string(),
    });
  }

  loop {
    let should_start = {
      let mut inner = state.inner.lock().await;
      if inner.booting || inner.process.is_some() {
        false
      } else {
        inner.booting = true;
        true
      }
    };

    if !should_start {
      sleep(PROBE_INTERVAL).await;
      continue;
    }

    let result = start_opencode(&app, state.clone(), &url).await;

    {
      let mut inner = state.inner.lock().await;
      inner.booting = false;
    }

    return result;
  }
}

async fn wait_for_existing_server(app: &AppHandle, state: AppState, url: &str) -> Result<BootstrapResponse, BootError> {
  let start = Instant::now();
  emit_status(
    app,
    "waiting",
    "Waiting for OpenCode",
    Some("Another OpenCode UI instance is starting the server.".to_string()),
  );

  loop {
    if probe_server(&state.client, url).await.unwrap_or(false) {
      emit_status(
        app,
        "ready",
        "OpenCode is ready",
        Some("The existing launch finished successfully.".to_string()),
      );
      return Ok(BootstrapResponse {
        url: url.to_string(),
        source: "existing".to_string(),
      });
    }

    if start.elapsed() > STARTUP_TIMEOUT {
      return Err(BootError::Message(format!(
        "OpenCode did not answer within {} seconds.",
        STARTUP_TIMEOUT.as_secs()
      )));
    }

    sleep(PROBE_INTERVAL).await;
  }
}

async fn start_opencode(app: &AppHandle, state: AppState, url: &str) -> Result<BootstrapResponse, BootError> {
  let config = {
    let inner = state.inner.lock().await;
    inner.config.clone()
  };

  emit_status(
    app,
    "resolving",
    "Locating OpenCode",
    Some("Searching PATH and common macOS locations.".to_string()),
  );

  let lock_file = match acquire_launch_lock() {
    Ok(file) => file,
    Err(_) => {
      return wait_for_existing_server(app, state, url).await;
    }
  };

  if probe_server(&state.client, url).await.unwrap_or(false) {
    return Ok(BootstrapResponse {
      url: url.to_string(),
      source: "existing".to_string(),
    });
  }

  let binary = resolve_binary(&config)?;
  let opener_shims = prepare_opener_shims()?;

  emit_status(
    app,
    "starting",
    "Starting OpenCode",
    Some(format!("Launching {}", binary.display())),
  );

  let mut command = Command::new(&binary);
  command.arg("web");
  command.args(&config.extra_args);
  command.stdin(std::process::Stdio::null());
  command.stdout(std::process::Stdio::piped());
  command.stderr(std::process::Stdio::piped());

  if let Some(dir) = &config.working_dir {
    command.current_dir(dir);
  }

  if let Some(password) = &config.server_password {
    command.env("OPENCODE_SERVER_PASSWORD", password);
  }

  command.env("BROWSER", opener_shims.join("open"));
  command.env("PATH", prepend_path(&opener_shims));

  let mut child = command
    .spawn()
    .map_err(|err| BootError::Message(format!("Failed to launch OpenCode: {err}")))?;

  let stdout = child.stdout.take();
  let stderr = child.stderr.take();

  {
    let mut inner = state.inner.lock().await;
    inner.process = Some(ManagedProcess {
      child,
      _lock_file: lock_file,
      started_at: Instant::now(),
    });
  }

  if let Some(stdout) = stdout {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
      let mut lines = BufReader::new(stdout).lines();
      while let Ok(Some(line)) = lines.next_line().await {
        emit_log(&app, "stdout", line);
      }
    });
  }

  if let Some(stderr) = stderr {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
      let mut lines = BufReader::new(stderr).lines();
      while let Ok(Some(line)) = lines.next_line().await {
        emit_log(&app, "stderr", line);
      }
    });
  }

  emit_status(
    app,
    "waiting",
    "Waiting for OpenCode",
    Some(format!("Waiting for {url} to respond.")),
  );

  let start = Instant::now();
  loop {
    if probe_server(&state.client, url).await.unwrap_or(false) {
      emit_status(
        app,
        "ready",
        "OpenCode is ready",
        Some("Loading the local OpenCode web UI.".to_string()),
      );
      spawn_exit_monitor(app.clone(), state.clone(), url.to_string());
      return Ok(BootstrapResponse {
        url: url.to_string(),
        source: "spawned".to_string(),
      });
    }

    {
      let mut inner = state.inner.lock().await;
      if let Some(process) = inner.process.as_mut() {
        match process.child.try_wait() {
          Ok(Some(status)) => {
            let elapsed = process.started_at.elapsed();
            let code = status
              .code()
              .map(|value| value.to_string())
              .unwrap_or_else(|| "signal".to_string());
            inner.process = None;
            return Err(BootError::Message(format!(
              "OpenCode exited after {elapsed:?} with status {code}."
            )));
          }
          Ok(None) => {}
          Err(err) => {
            inner.process = None;
            return Err(BootError::Message(format!(
              "Failed to inspect OpenCode process: {err}"
            )));
          }
        }
      }
    }

    if start.elapsed() > STARTUP_TIMEOUT {
      if let Err(err) = shutdown_process(state.clone()).await {
        emit_log(app, "stderr", format!("cleanup after timeout failed: {err}"));
      }
      return Err(BootError::Message(format!(
        "OpenCode did not answer within {} seconds.",
        STARTUP_TIMEOUT.as_secs()
      )));
    }

    sleep(PROBE_INTERVAL).await;
  }
}

async fn shutdown_process(state: AppState) -> Result<(), BootError> {
  let process = {
    let mut inner = state.inner.lock().await;
    inner.process.take()
  };

  if let Some(mut process) = process {
    let _ = process.child.start_kill();
    let _ = process.child.wait().await;
  }

  Ok(())
}

fn acquire_launch_lock() -> Result<fs::File, BootError> {
  let Some(base) = dirs::data_local_dir() else {
    return Err(BootError::Message("Unable to resolve a local data directory.".to_string()));
  };

  let dir = base.join("opencode-ui");
  fs::create_dir_all(&dir).map_err(|err| BootError::Message(format!(
    "Failed to create app data directory: {err}"
  )))?;

  let file = fs::OpenOptions::new()
    .create(true)
    .read(true)
    .write(true)
    .open(dir.join("launch.lock"))
    .map_err(|err| BootError::Message(format!("Failed to open launch lock: {err}")))?;

  file.try_lock_exclusive().map_err(|err| {
    BootError::Message(format!("Another OpenCode UI instance is already managing the server: {err}"))
  })?;

  Ok(file)
}

fn prepend_path(prefix: &Path) -> String {
  let current = env::var_os("PATH").unwrap_or_default();
  let mut parts = vec![prefix.as_os_str().to_os_string()];
  if !current.is_empty() {
    parts.push(current);
  }
  env::join_paths(parts).map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|_| prefix.to_string_lossy().to_string())
}

fn prepare_opener_shims() -> Result<PathBuf, BootError> {
  let Some(base) = dirs::data_local_dir() else {
    return Err(BootError::Message("Unable to resolve a local data directory.".to_string()));
  };

  let dir = base.join("opencode-ui").join("opener-shims");
  fs::create_dir_all(&dir).map_err(|err| BootError::Message(format!(
    "Failed to create browser shim directory: {err}"
  )))?;

  create_noop_shim(&dir.join("open"))?;
  create_noop_shim(&dir.join("xdg-open"))?;
  #[cfg(windows)]
  {
    create_noop_shim(&dir.join("start"))?;
  }

  Ok(dir)
}

fn create_noop_shim(path: &Path) -> Result<(), BootError> {
  #[cfg(windows)]
  let script = b"@echo off\r\nexit /b 0\r\n";

  #[cfg(not(windows))]
  let script = b"#!/bin/sh\nexit 0\n";

  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|err| BootError::Message(format!(
      "Failed to prepare opener shim directory: {err}"
    )))?;
  }

  fs::write(path, script).map_err(|err| BootError::Message(format!(
    "Failed to write opener shim {}: {err}",
    path.display()
  )))?;

  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
      .map_err(|err| BootError::Message(format!("Failed to inspect opener shim {}: {err}", path.display())))?
      .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(|err| BootError::Message(format!(
      "Failed to mark opener shim executable {}: {err}",
      path.display()
    )))?;
  }

  Ok(())
}

fn spawn_exit_monitor(app: AppHandle, state: AppState, url: String) {
  tauri::async_runtime::spawn(async move {
    loop {
      sleep(MONITOR_INTERVAL).await;

      let exit = {
        let mut inner = state.inner.lock().await;
        let Some(process) = inner.process.as_mut() else {
          return;
        };

        match process.child.try_wait() {
          Ok(Some(status)) => {
            let elapsed = process.started_at.elapsed();
            inner.process = None;
            Some((status.code(), elapsed))
          }
          Ok(None) => None,
          Err(err) => {
            inner.process = None;
            emit_log(&app, "stderr", format!("OpenCode monitor failed: {err}"));
            return;
          }
        }
      };

      if let Some((code, elapsed)) = exit {
        let code_text = code
          .map(|value| value.to_string())
          .unwrap_or_else(|| "signal".to_string());
        emit_status(
          &app,
          "error",
          "OpenCode stopped unexpectedly",
          Some(format!("The OpenCode server at {url} exited after {elapsed:?} with status {code_text}.")),
        );
        return;
      }
    }
  });
}

fn cleanup(app: &AppHandle) {
  let state_ref = app.state::<AppState>();
  let state = AppState {
    inner: state_ref.inner.clone(),
    client: state_ref.client.clone(),
  };
  let _ = tauri::async_runtime::block_on(async move { shutdown_process(state).await });
}

fn spawn_update_check(app: AppHandle) {
  tauri::async_runtime::spawn(async move {
    let updater = match app.updater() {
      Ok(updater) => updater,
      Err(err) => {
        emit_log(&app, "stderr", format!("update checker unavailable: {err}"));
        return;
      }
    };

    match updater.check().await {
      Ok(Some(update)) => {
        emit_log(&app, "stdout", format!("update available: {} -> {}", update.current_version, update.version));

        if let Err(err) = update
          .download_and_install(
            |_, _| {},
            || {},
          )
          .await
        {
          emit_log(&app, "stderr", format!("update install failed: {err}"));
          return;
        }

        app.restart();
      }
      Ok(None) => {
        emit_log(&app, "stdout", "no update available".to_string());
      }
      Err(err) => {
        emit_log(&app, "stderr", format!("update check failed: {err}"));
      }
    }
  });
}

fn main() {
  let config = load_config();
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(3))
    .build()
    .expect("reqwest client");

  let state = AppState {
    inner: Arc::new(Mutex::new(State {
      config,
      process: None,
      booting: false,
    })),
    client,
  };

  tauri::Builder::default()
    .manage(state)
    .setup(|app| {
      let config = app.state::<AppState>().inner.blocking_lock().config.clone();
      let port = config.port;
      let handle = app.handle();
      #[cfg(desktop)]
      handle.plugin(tauri_plugin_window_state::Builder::default().build())?;
      emit_status(handle, "checking", "Checking OpenCode", Some(format!("Local server target: {}.", local_url(port))));
      spawn_update_check(app.handle().clone());
      Ok(())
    })
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .on_window_event(|window, event| {
      if matches!(event, WindowEvent::CloseRequested { .. }) {
        cleanup(&window.app_handle());
      }
    })
    .invoke_handler(tauri::generate_handler![bootstrap, set_binary_path])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
