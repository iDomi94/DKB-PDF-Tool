const { invoke } = window.__TAURI__.core;

let current = null; // aktuell im Formular stehende Einstellungen (ungespeichert moeglich)

function el(id) {
  return document.querySelector('#' + id);
}

/** Baut eine Zeile mit zwei Textfeldern + Löschen-Button. */
function patternRow(container, { left, right, onRemove }, placeholders) {
  const row = document.createElement('div');
  row.className = 'pattern-row';

  const leftInput = document.createElement('input');
  leftInput.type = 'text';
  leftInput.value = left;
  leftInput.placeholder = placeholders[0];

  const rightInput = document.createElement('input');
  rightInput.type = 'text';
  rightInput.value = right;
  rightInput.placeholder = placeholders[1];

  const removeBtn = document.createElement('button');
  removeBtn.type = 'button';
  removeBtn.textContent = '×';
  removeBtn.title = 'Entfernen';
  removeBtn.addEventListener('click', () => {
    row.remove();
    onRemove();
    schedulePreview();
  });

  leftInput.addEventListener('input', schedulePreview);
  rightInput.addEventListener('input', schedulePreview);

  row.append(leftInput, rightInput, removeBtn);
  container.appendChild(row);
  return { row, leftInput, rightInput };
}

function renderPatternList(containerId, items, placeholders, readBack) {
  const container = el(containerId);
  container.innerHTML = '';
  for (const item of items) {
    patternRow(
      container,
      { left: item[readBack.leftKey], right: item[readBack.rightKey], onRemove: () => {} },
      placeholders
    );
  }
  container._readBack = readBack;
}

function readPatternList(containerId) {
  const container = el(containerId);
  const { leftKey, rightKey } = container._readBack;
  return [...container.querySelectorAll('.pattern-row')].map((row) => {
    const [leftInput, rightInput] = row.querySelectorAll('input');
    return { [leftKey]: leftInput.value, [rightKey]: rightInput.value };
  });
}

function addPatternRow(containerId, placeholders, readBack, item = { [readBack.leftKey]: '', [readBack.rightKey]: '' }) {
  const container = el(containerId);
  patternRow(container, { left: item[readBack.leftKey], right: item[readBack.rightKey], onRemove: () => {} }, placeholders);
  schedulePreview();
}

function renderForm(settings) {
  current = settings;
  el('output-dir').value = settings.outputDir ?? '';
  el('path-template').value = settings.pathTemplate;
  el('filename-template').value = settings.filenameTemplate;

  renderPatternList('category-patterns', settings.categoryPatterns, ['Regex, z. B. ^Kontoauszug', 'Ordnername'], {
    leftKey: 'pattern',
    rightKey: 'label',
  });
  renderPatternList('sub-patterns', settings.securitiesSubPatterns, ['Regex, z. B. ^Abrechnung Kauf', 'Ordnername'], {
    leftKey: 'pattern',
    rightKey: 'label',
  });
  renderPatternList('depot-owners', settings.depotOwners, ['Depotnummer', 'Name'], {
    leftKey: 'depotNumber',
    rightKey: 'name',
  });

  schedulePreview();
}

function collectFormAsSettings() {
  return {
    outputDir: el('output-dir').value.trim() || null,
    pathTemplate: el('path-template').value,
    filenameTemplate: el('filename-template').value,
    categoryPatterns: readPatternList('category-patterns'),
    securitiesSubPatterns: readPatternList('sub-patterns'),
    depotOwners: readPatternList('depot-owners'),
  };
}

let previewTimer = null;
function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(runPreview, 150);
}

async function runPreview() {
  const settings = collectFormAsSettings();
  const sample = {
    id: el('preview-id').value.trim() || 'beispiel-id',
    title: el('preview-title').value,
    date: el('preview-date').value,
    depotNumber: el('preview-depot').value.trim() || null,
    accountRef: el('preview-account').value.trim() || null,
  };
  try {
    const result = await invoke('preview_document', { settings, sample });
    const parts = [
      `Kategorie: ${result.category ?? '(kein Treffer -> "unbekannt" beim echten Lauf)'}`,
      result.owner ? `Besitzer: ${result.owner}` : null,
      result.subCategory ? `Unterart: ${result.subCategory}` : null,
      `Pfad: ${result.pathSegments.join(' / ')}`,
      `Dateiname: ${result.filename}`,
    ].filter(Boolean);
    el('preview-result').textContent = parts.join('\n');
  } catch (err) {
    el('preview-result').textContent = 'Fehler in der Vorschau: ' + err;
  }
}

async function loadSettings() {
  const settings = await invoke('get_settings');
  renderForm(settings);
}

async function saveSettings() {
  const settings = collectFormAsSettings();
  try {
    await invoke('save_settings', { settings });
    current = settings;
    el('settings-status').textContent = 'Gespeichert.';
  } catch (err) {
    el('settings-status').textContent = 'Fehler: ' + err;
  }
}

async function resetSettings() {
  const defaults = await invoke('default_settings');
  renderForm(defaults);
  el('settings-status').textContent = 'Zurückgesetzt (noch nicht gespeichert).';
}

export function initSettingsTab() {
  el('pick-output-dir').addEventListener('click', async () => {
    const folder = await invoke('pick_folder');
    if (folder) el('output-dir').value = folder;
  });

  el('add-category-pattern').addEventListener('click', () =>
    addPatternRow('category-patterns', ['Regex, z. B. ^Kontoauszug', 'Ordnername'], { leftKey: 'pattern', rightKey: 'label' })
  );
  el('add-sub-pattern').addEventListener('click', () =>
    addPatternRow('sub-patterns', ['Regex, z. B. ^Abrechnung Kauf', 'Ordnername'], { leftKey: 'pattern', rightKey: 'label' })
  );
  el('add-depot-owner').addEventListener('click', () =>
    addPatternRow('depot-owners', ['Depotnummer', 'Name'], { leftKey: 'depotNumber', rightKey: 'name' })
  );

  el('save-settings').addEventListener('click', saveSettings);
  el('reset-settings').addEventListener('click', resetSettings);

  for (const id of ['path-template', 'filename-template', 'preview-title', 'preview-date', 'preview-depot', 'preview-account', 'preview-id']) {
    el(id).addEventListener('input', schedulePreview);
  }

  loadSettings();
}
