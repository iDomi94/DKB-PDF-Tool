import { initSettingsTab } from './settings.js';
import { initSortTab } from './sort.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// --- Tabs -------------------------------------------------------------

for (const btn of document.querySelectorAll('.tab-btn')) {
  btn.addEventListener('click', () => {
    for (const b of document.querySelectorAll('.tab-btn')) b.classList.remove('active');
    for (const p of document.querySelectorAll('.tab-panel')) p.classList.remove('active');
    btn.classList.add('active');
    document.querySelector('#tab-' + btn.dataset.tab).classList.add('active');
  });
}

// --- Online-Abruf -------------------------------------------------------

const statusEl = document.querySelector('#status-msg');
const logEl = document.querySelector('#log');

function appendLog(text) {
  logEl.textContent += text + '\n';
  logEl.scrollTop = logEl.scrollHeight;
}

function setStatus(text) {
  statusEl.textContent = text;
}

listen('sidecar-event', (event) => {
  appendLog('EVENT: ' + event.payload);
  let msg;
  try {
    msg = JSON.parse(event.payload);
  } catch {
    return;
  }
  switch (msg.type) {
    case 'ready':
      setStatus('Sidecar bereit.');
      break;
    case 'status':
      setStatus(msg.message);
      break;
    case 'logged_in':
      setStatus('Angemeldet.');
      break;
    case 'browser_closed':
      setStatus('Browser geschlossen.');
      break;
    case 'error':
      setStatus('Fehler: ' + msg.message);
      break;
  }
});

listen('sidecar-exited', () => {
  appendLog('--- Sidecar-Prozess beendet ---');
});

document.querySelector('#login-btn').addEventListener('click', async () => {
  setStatus('Starte Anmeldung ...');
  try {
    await invoke('sidecar_send', { cmd: 'login' });
  } catch (err) {
    setStatus('Fehler: ' + err);
  }
});

document.querySelector('#quit-btn').addEventListener('click', async () => {
  try {
    await invoke('sidecar_send', { cmd: 'quit' });
  } catch (err) {
    setStatus('Fehler: ' + err);
  }
});

// --- Download + Fortschritt ------------------------------------------------

const progressEl = document.querySelector('#download-progress');
const downloadBtn = document.querySelector('#download-btn');
const pauseBtn = document.querySelector('#pause-btn');
const cancelBtn = document.querySelector('#cancel-btn');

function setProgress(text) {
  progressEl.textContent = text;
}

/**
 * Laeuft gerade ein Abruf? Steuert, welche Buttons sichtbar sind.
 * Sperrt zusaetzlich den Sortieren-Start -- beide teilen sich denselben
 * Pause/Abbrechen-Zustand in Rust (DownloadControl) und duerfen deshalb
 * nicht gleichzeitig laufen.
 */
function setRunning(running) {
  downloadBtn.disabled = running;
  pauseBtn.hidden = !running;
  cancelBtn.hidden = !running;
  if (!running) {
    pauseBtn.textContent = 'Pause';
  }
  const sortBtn = document.querySelector('#sort-btn');
  if (sortBtn) sortBtn.disabled = running;
}

listen('download-progress', (event) => {
  const p = event.payload;
  switch (p.phase) {
    case 'pending':
      setProgress(`0 / ${p.total} verarbeitet`);
      setRunning(true);
      break;
    case 'progress':
      setProgress(`${p.done} / ${p.total} verarbeitet -- zuletzt: ${p.title}`);
      break;
    case 'skipped':
      setProgress(`${p.done} / ${p.total} verarbeitet -- übersprungen: ${p.title}`);
      break;
    case 'item_error':
      appendLog(`FEHLER bei "${p.title}": ${p.message}`);
      break;
    case 'paused':
      setProgress('Pausiert.');
      break;
    case 'cancelled':
      setProgress(`Abgebrochen bei ${p.done} / ${p.total}.`);
      setRunning(false);
      break;
    case 'aborted':
      setProgress('Abgebrochen: ' + p.message);
      setRunning(false);
      break;
    case 'done':
      setProgress(`Fertig. ${p.done} von ${p.total} neu geladen.`);
      setRunning(false);
      break;
  }
});

let isPaused = false;
pauseBtn.addEventListener('click', async () => {
  isPaused = !isPaused;
  pauseBtn.textContent = isPaused ? 'Fortsetzen' : 'Pause';
  try {
    await invoke(isPaused ? 'pause_download' : 'resume_download');
  } catch (err) {
    appendLog('Fehler bei Pause/Fortsetzen: ' + err);
  }
});

cancelBtn.addEventListener('click', async () => {
  try {
    await invoke('cancel_download');
  } catch (err) {
    appendLog('Fehler bei Abbrechen: ' + err);
  }
});

downloadBtn.addEventListener('click', async () => {
  setProgress('Starte Abruf ...');
  isPaused = false;
  try {
    await invoke('start_download');
  } catch (err) {
    setProgress('Fehler: ' + err);
    setRunning(false);
  }
});

// --- Duplikat-Dialog --------------------------------------------------------

const dupDialog = document.querySelector('#duplicate-dialog');
const dupInfo = document.querySelector('#duplicate-info');

listen('duplicate-found', (event) => {
  const d = event.payload;
  const sizeInfo = d.identical
    ? 'Inhalt ist identisch mit der vorhandenen Datei.'
    : `Größe vorhanden: ${d.existingSize} Byte, neu: ${d.newSize} Byte -- unterschiedlicher Inhalt.`;
  dupInfo.textContent = `„${d.title}“ existiert schon unter:\n${d.path}\n\n${sizeInfo}`;
  dupDialog.hidden = false;
});

function resolveDuplicate(action) {
  dupDialog.hidden = true;
  invoke('resolve_duplicate', { action }).catch((err) => appendLog('Fehler bei Duplikat-Antwort: ' + err));
}

document.querySelector('#dup-skip').addEventListener('click', () => resolveDuplicate('Skip'));
document.querySelector('#dup-overwrite').addEventListener('click', () => resolveDuplicate('Overwrite'));
document.querySelector('#dup-keep-both').addEventListener('click', () => resolveDuplicate('KeepBoth'));

// --- Sortieren --------------------------------------------------------

initSortTab();

// --- Einstellungen --------------------------------------------------------

initSettingsTab();
