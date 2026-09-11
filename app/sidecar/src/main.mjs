/**
 * Sidecar-Einstiegspunkt. Ein einziger, langlebiger Prozess pro Sitzung
 * (nicht einer pro Kommando!) -- der Grund: Chrome legt beim Start eine
 * SingletonLock-Datei im Profilordner an. Zwei Prozesse, die gleichzeitig
 * dasselbe Profil oeffnen, kollidieren damit (siehe HANDOFF.md im
 * DKB_Dateiabruf-Projekt, dort live erlebt). Der Tauri-Hauptprozess startet
 * diesen Sidecar deshalb einmal und schickt ueber die gesamte Sitzung
 * hinweg mehrere Kommandos per stdin -- nicht: fuer jedes Kommando einen
 * neuen Sidecar-Prozess.
 *
 * Protokoll: eine JSON-Zeile pro Kommando/Ereignis.
 *   stdin  (Rust -> hier):
 *     {"cmd": "login"} | {"cmd": "quit"}
 *     {"cmd": "list"}              -> einmal die volle Dokumentenliste
 *     {"cmd": "fetch", "id": ".."} -> ein einzelnes Dokument laden
 *   stdout (hier -> Rust):
 *     {"type": "ready"} | {"type": "status", ...}
 *     {"type": "logged_in", ...} | {"type": "error", ...}
 *     {"type": "list_result", "items": [...]}
 *     {"type": "document", "id", "contentType", "contentBase64"}
 *     {"type": "document_error", "id", "message"}
 *
 * Der Sidecar entscheidet bewusst NICHT ueber Zielpfade oder Kategorien
 * -- das macht der Rust-Hauptprozess (einzige Quelle der Wahrheit dafuer,
 * siehe src-tauri/src/categorize.rs). Der Sidecar liefert nur rohe
 * Metadaten und Dokument-Bytes.
 *
 * stdout ist ausschliesslich fuer dieses Protokoll reserviert -- jede
 * Diagnose-Ausgabe geht auf stderr (siehe api.mjs).
 */
import readline from 'node:readline';
import os from 'node:os';
import path from 'node:path';
import { openContext, firstPage, randomDelay } from './browser.mjs';
import { ApiSession } from './api.mjs';
import * as dkb from './dkbApi.mjs';

const session = { context: null, page: null, api: null };

function send(event) {
  process.stdout.write(JSON.stringify(event) + '\n');
}

const status = (message) => send({ type: 'status', message });
const fail = (message) => send({ type: 'error', message });

// Fuer M1 hartkodiert. Wandert in M2 in eine gemeinsame Config-Datei, die
// der Tauri-Hauptprozess verwaltet und beim "login"-Kommando mitschickt.
const DEFAULT_CFG = {
  profileDir: path.join(os.homedir(), '.dkb-postfach-app', 'profile'),
  channel: 'chrome',
  locale: 'de-DE',
  timezone: 'Europe/Berlin',
  viewport: { width: 1440, height: 900 },
  startUrl: 'https://banking.dkb.de/',
};

const LOGIN_TIMEOUT_MS = 5 * 60_000;

async function handleLogin(cfg) {
  if (session.context) {
    status('Browser ist bereits offen.');
    return;
  }
  try {
    status('Öffne Browser ...');
    session.context = await openContext(cfg);
    session.page = await firstPage(session.context);
    session.api = new ApiSession(session.page);

    session.context.on('close', () => {
      session.context = null;
      session.page = null;
      session.api = null;
      send({ type: 'browser_closed' });
    });

    await session.page.goto(cfg.startUrl, { waitUntil: 'domcontentloaded' });
    status('Browser geöffnet -- bitte im Fenster anmelden.');

    // Nicht (nur) per URL erkennen: die SPA leitet zwar auf /login um,
    // aber erst nach einem Moment -- direkt nach domcontentloaded ist das
    // Passwortfeld oft noch nicht im DOM. Ein einzelner Check zu frueh
    // meldet dann faelschlich "angemeldet" (kein Feld gefunden, weil
    // noch nichts gerendert ist). Deshalb: kurze Anlaufzeit, danach zwei
    // aufeinanderfolgende Checks ohne Passwortfeld verlangen.
    await session.page.waitForTimeout(1500);
    if (!session.page) return; // Waehrend der Anlaufzeit geschlossen worden.

    const deadline = Date.now() + LOGIN_TIMEOUT_MS;
    let consecutiveNoPassword = 0;
    while (Date.now() < deadline) {
      if (!session.page) return; // Browser wurde inzwischen geschlossen.
      const passwordVisible = await session.page
        .locator('input[type="password"]')
        .first()
        .isVisible()
        .catch(() => false);
      if (!passwordVisible) {
        consecutiveNoPassword++;
        if (consecutiveNoPassword >= 2) {
          send({ type: 'logged_in', url: session.page.url() });
          return;
        }
      } else {
        consecutiveNoPassword = 0;
      }
      await session.page.waitForTimeout(1000);
    }
    fail('Zeitüberschreitung: keine Anmeldung innerhalb von 5 Minuten erkannt.');
  } catch (err) {
    // Browser waehrenddessen geschlossen (durch "quit" oder von Hand)?
    // Dann kommt ohnehin schon ein "browser_closed"-Ereignis vom
    // close-Handler -- kein zusaetzlicher Fehler noetig. Die Reihenfolge
    // von Promise-Rejection und close-Event ist nicht garantiert, daher
    // hier unabhaengig von session.context pruefen.
    if (/Target page, context or browser has been closed/i.test(err.message)) return;
    fail(`Login fehlgeschlagen: ${err.message}`);
  }
}

async function handleQuit() {
  if (session.context) {
    try {
      await session.context.close();
    } catch {
      // Bereits weg -- egal.
    }
  }
  send({ type: 'bye' });
  process.exit(0);
}

async function handleList() {
  if (!session.api) {
    fail('Nicht angemeldet -- zuerst "login" ausfuehren.');
    return;
  }
  try {
    const items = [];
    for (let offset = 0; offset < 20_000; offset += dkb.PAGE_SIZE) {
      const response = await session.api.get(dkb.listUrl(dkb.PAGE_SIZE, offset));
      if (!response.ok()) {
        fail(`Liste offset=${offset} -> HTTP ${response.status()}`);
        return;
      }
      const json = await response.json();
      const batch = Array.isArray(json.data) ? json.data : [];
      if (batch.length === 0) break;

      for (const raw of batch) items.push(dkb.extractItem(raw));
      status(`${items.length} Einträge gelesen ...`);
      if (batch.length < dkb.PAGE_SIZE) break;
    }
    send({ type: 'list_result', items });
  } catch (err) {
    fail(`Liste fehlgeschlagen: ${err.message}`);
  }
}

async function handleFetch(id) {
  if (!session.api) {
    fail('Nicht angemeldet -- zuerst "login" ausfuehren.');
    return;
  }
  try {
    const response = await session.api.get(dkb.docUrl(id), { extra: { accept: dkb.DOC_ACCEPT } });
    if (!response.ok()) throw new Error(`HTTP ${response.status()}`);

    const contentType = (response.headers()['content-type'] ?? '').split(';')[0];
    const body = await response.body();
    if (body.length === 0) throw new Error('Leere Antwort');
    // Content-Type allein reicht nicht als Beweis (siehe DKB_Dateiabruf/
    // HANDOFF.md) -- ein Server, der application/pdf meldet und trotzdem
    // JSON schickt, ist keine Seltenheit.
    if (contentType.includes('json') || body.subarray(0, 1).toString('latin1') === '{') {
      throw new Error('Endpunkt liefert JSON statt Dokument.');
    }

    send({ type: 'document', id, contentType, contentBase64: body.toString('base64') });
  } catch (err) {
    send({ type: 'document_error', id, message: err.message });
  }
  // Freundlich bleiben: kleine Pause, bevor der Sidecar fuer das naechste
  // Kommando bereit ist (Rust wartet ohnehin auf dieses Ereignis, bevor
  // es das naechste "fetch" schickt).
  await randomDelay([250, 700]);
}

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  if (!line.trim()) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    fail('Ungültiges JSON empfangen.');
    return;
  }

  switch (msg.cmd) {
    case 'login':
      handleLogin({ ...DEFAULT_CFG, ...(msg.config ?? {}) });
      break;
    case 'list':
      handleList();
      break;
    case 'fetch':
      handleFetch(msg.id);
      break;
    case 'quit':
      handleQuit();
      break;
    default:
      fail(`Unbekanntes Kommando: ${msg.cmd}`);
  }
});

send({ type: 'ready' });
