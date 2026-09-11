/**
 * Uebernommen aus DKB_Dateiabruf/lib/api.mjs (dort ausfuehrlich
 * dokumentiert und gegen die echte DKB-API getestet). Einzige Aenderung:
 * Diagnose-Ausgaben auf stderr statt stdout -- stdout ist hier exklusiv
 * fuer das JSON-Zeilen-Protokoll mit dem Tauri-Hauptprozess reserviert.
 */

const INTERESTING = /\/api\//;

function relevantHeaders(headers) {
  const out = {};
  for (const [key, value] of Object.entries(headers)) {
    const k = key.toLowerCase();
    if (k === 'authorization' || k === 'accept' || k.startsWith('x-')) {
      out[k] = value;
    }
  }
  return out;
}

export class ApiSession {
  constructor(page, { debug = false } = {}) {
    this.page = page;
    this.debug = debug;
    this.headers = null;
    this._attach();
  }

  _attach() {
    this.page.on('request', (request) => {
      const url = request.url();
      if (!INTERESTING.test(url)) return;
      const headers = request.headers();
      if (!headers.authorization) return;
      this.headers = relevantHeaders(headers);
      if (this.debug) console.error(`    [auth] Token aufgeschnappt von ${url}`);
    });
  }

  /**
   * Wartet kurz, ob die Anwendung einen Token verschickt. Kein Fehler,
   * wenn nicht -- kann durchaus sein, dass keiner kommt, weil der genutzte
   * Endpunkt keinen braucht.
   */
  async waitForToken(timeoutMs = 8_000) {
    const deadline = Date.now() + timeoutMs;
    while (!this.headers && Date.now() < deadline) {
      await this.page.waitForTimeout(400);
    }
    return this.headers;
  }

  /** Nur bei echtem 401/403: Seite neu laden, kurz auf frischen Token warten. */
  async reauth() {
    console.error('    [auth] 401/403 -- lade Seite neu und warte kurz auf einen Token ...');
    this.headers = null;
    await this.page.reload({ waitUntil: 'domcontentloaded' });
    await this.waitForToken();
  }

  /**
   * GET. Nutzt einen ggf. bereits aufgeschnappten Token, verlangt aber
   * keinen -- viele Endpunkte kommen mit der Session-Cookie alleine aus
   * (die Playwright automatisch mitschickt). Bei 401/403 einmal neu
   * authentifizieren und wiederholen.
   *
   * `extra.accept` ueberschreibt den Accept-Header der SPA -- JSON:API
   * entscheidet darueber, ob eine Dokument-URL Metadaten oder den
   * eigentlichen Inhalt liefert.
   */
  async get(url, { retried = false, extra = {} } = {}) {
    const headers = { ...(this.headers ?? {}) };
    if (extra.accept) headers.accept = extra.accept;

    const response = await this.page.request.get(url, {
      headers,
      failOnStatusCode: false,
    });

    if ((response.status() === 401 || response.status() === 403) && !retried) {
      await this.reauth();
      return this.get(url, { retried: true, extra });
    }
    return response;
  }
}
