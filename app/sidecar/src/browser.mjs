/**
 * Browser-Lebenszyklus. Uebernommen aus DKB_Dateiabruf/lib/browser.mjs,
 * angepasst auf playwright-core (kein eigenes Chromium wird geladen --
 * es wird ausschliesslich das vorhandene System-Chrome gestartet).
 */
import { chromium } from 'playwright-core';
import path from 'node:path';
import fs from 'node:fs/promises';

/**
 * Oeffnet einen persistenten Browser-Kontext. Persistent heisst: eigenes
 * Nutzerprofil auf der Platte, inklusive IndexedDB -- ohne das verliert
 * man die Geraetebindung der Bank bei jedem Lauf.
 *
 * Kein Fallback auf ein gebuendeltes Chromium: playwright-core bringt
 * keins mit. Ist der `channel` (z. B. "chrome") nicht installiert,
 * schlaegt das mit einer klaren Fehlermeldung fehl.
 */
export async function openContext(cfg) {
  const userDataDir = path.resolve(cfg.profileDir);
  await fs.mkdir(userDataDir, { recursive: true });

  return chromium.launchPersistentContext(userDataDir, {
    headless: false,
    channel: cfg.channel ?? 'chrome',
    locale: cfg.locale,
    timezoneId: cfg.timezone,
    viewport: cfg.viewport,
    acceptDownloads: true,
    args: ['--disable-blink-features=AutomationControlled'],
  });
}

/** Erste offene Seite wiederverwenden statt eine zweite aufzumachen. */
export function firstPage(context) {
  return context.pages()[0] ?? context.newPage();
}

export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

export function randomDelay([min, max]) {
  return sleep(min + Math.random() * (max - min));
}
