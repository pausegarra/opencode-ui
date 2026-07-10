import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

const phasePill = document.querySelector('#phase-pill');
const statusCopy = document.querySelector('#status-copy');
const detailCopy = document.querySelector('#detail-copy');
const progressFill = document.querySelector('#progress-fill');
const logsEl = document.querySelector('#logs');
const errorPanel = document.querySelector('#error-panel');
const errorMessage = document.querySelector('#error-message');
const retryButton = document.querySelector('#retry-button');
const usePathButton = document.querySelector('#use-path-button');
const browseButton = document.querySelector('#browse-button');
const binaryPathInput = document.querySelector('#binary-path');
const toggleLogsButton = document.querySelector('#toggle-logs');
const logPanel = document.querySelector('.log-panel');

const phases = {
  checking: { label: 'Checking', width: '20%' },
  resolving: { label: 'Locating', width: '35%' },
  starting: { label: 'Starting', width: '55%' },
  downloading: { label: 'Updating', width: '60%' },
  installing: { label: 'Installing', width: '80%' },
  restarting: { label: 'Restarting', width: '95%' },
  waiting: { label: 'Waiting', width: '75%' },
  ready: { label: 'Ready', width: '100%' },
  error: { label: 'Error', width: '12%' }
};

let currentPhase = 'checking';
let unlistenStatus = null;
let unlistenLog = null;

function setPhase(phase, message) {
  currentPhase = phase;
  const config = phases[phase] ?? phases.checking;
  phasePill.textContent = config.label;
  statusCopy.textContent = message;
  progressFill.style.width = config.width;
}

function appendLog(line) {
  if (!line) {
    return;
  }

  logsEl.textContent += `${line}\n`;
  logsEl.scrollTop = logsEl.scrollHeight;
}

function showError(message) {
  setPhase('error', 'OpenCode needs attention');
  detailCopy.textContent = 'Use a custom path, then retry.';
  errorPanel.classList.remove('hidden');
  errorMessage.textContent = message;
}

async function boot() {
  errorPanel.classList.add('hidden');
  errorMessage.textContent = '';
  setPhase('checking', 'Checking for OpenCode');
  detailCopy.textContent = 'Finding the binary and probing the local server.';

  try {
    if (!unlistenStatus) {
      unlistenStatus = await listen('opencode://status', (event) => {
        const payload = event.payload ?? {};
        if (payload.phase) {
          setPhase(payload.phase, payload.message ?? statusCopy.textContent);
        }
        if (payload.detail) {
          detailCopy.textContent = payload.detail;
        }
      });
    }

    if (!unlistenLog) {
      unlistenLog = await listen('opencode://log', (event) => {
        const payload = event.payload ?? {};
        appendLog(`[${payload.stream ?? 'system'}] ${payload.line ?? ''}`.trim());
      });
    }

    const result = await invoke('bootstrap');
    setPhase('ready', 'OpenCode is ready');
    detailCopy.textContent = 'Switching to the local OpenCode web UI.';

    if (result?.url) {
      window.location.replace(result.url);
    }
  } catch (error) {
    const message = typeof error === 'string' ? error : error?.message || String(error);
    showError(message);
  }
}

retryButton?.addEventListener('click', () => {
  boot();
});

usePathButton?.addEventListener('click', async () => {
  const binaryPath = binaryPathInput.value.trim();
  if (!binaryPath) {
    showError('Enter a full path to the opencode executable first.');
    return;
  }

  try {
    await invoke('set_binary_path', { path: binaryPath });
    appendLog(`[system] saved custom binary path: ${binaryPath}`);
    boot();
  } catch (error) {
    const message = typeof error === 'string' ? error : error?.message || String(error);
    showError(message);
  }
});

browseButton?.addEventListener('click', async () => {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
    });

    const path = Array.isArray(selected) ? selected[0] : selected;
    if (!path || typeof path !== 'string') {
      return;
    }

    binaryPathInput.value = path;
    await invoke('set_binary_path', { path });
    appendLog(`[system] selected binary: ${path}`);
  } catch (error) {
    const message = typeof error === 'string' ? error : error?.message || String(error);
    showError(message);
  }
});

toggleLogsButton?.addEventListener('click', () => {
  const collapsed = logPanel.classList.toggle('collapsed');
  toggleLogsButton.textContent = collapsed ? 'Show' : 'Hide';
});

boot();
