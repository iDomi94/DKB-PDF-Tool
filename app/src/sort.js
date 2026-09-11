const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function el(id) {
  return document.querySelector('#' + id);
}

const progressEl = el('sort-progress');
const logEl = el('sort-log');
const sortBtn = el('sort-btn');
const pauseBtn = el('sort-pause-btn');
const cancelBtn = el('sort-cancel-btn');
const inputDirEl = el('sort-input-dir');

function appendLog(text) {
  logEl.textContent += text + '\n';
  logEl.scrollTop = logEl.scrollHeight;
}

function setProgress(text) {
  progressEl.textContent = text;
}

/** Siehe main.js setRunning -- beide teilen sich denselben Pause/
 * Abbrechen-Zustand in Rust und duerfen nicht gleichzeitig laufen. */
function setRunning(running) {
  sortBtn.disabled = running;
  pauseBtn.hidden = !running;
  cancelBtn.hidden = !running;
  if (!running) pauseBtn.textContent = 'Pause';
  const downloadBtn = document.querySelector('#download-btn');
  if (downloadBtn) downloadBtn.disabled = running;
}

export function initSortTab() {
  el('pick-sort-input-dir').addEventListener('click', async () => {
    const folder = await invoke('pick_folder');
    if (folder) inputDirEl.value = folder;
  });

  listen('sort-progress', (event) => {
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
        setProgress(`Fertig. ${p.done} von ${p.total} sortiert.`);
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

  sortBtn.addEventListener('click', async () => {
    if (!inputDirEl.value) {
      setProgress('Bitte zuerst einen Quellordner wählen.');
      return;
    }
    setProgress('Starte Sortierung ...');
    isPaused = false;
    try {
      await invoke('sort_folder', { inputDir: inputDirEl.value });
    } catch (err) {
      setProgress('Fehler: ' + err);
      setRunning(false);
    }
  });
}
