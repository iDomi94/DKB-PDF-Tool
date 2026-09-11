/**
 * DKB-spezifische Endpunkte und Feld-Kandidaten. Bestaetigt gegen die
 * echte API (siehe DKB_Dateiabruf/HANDOFF.md, 11.09.2026) -- bewusst
 * NICHT ueber die Einstellungen konfigurierbar, anders als Kategorien/
 * Vorlagen: das hier ist Implementierungsdetail der DKB-Anbindung, keine
 * Nutzer-Einstellung.
 */

export const PAGE_SIZE = 200;

export function listUrl(limit, offset) {
  return (
    'https://banking.dkb.de/api/documentstorage/documents' +
    `?page%5Blimit%5D=${limit}&page%5Boffset%5D=${offset}&sort=-creationDate`
  );
}

export function docUrl(id) {
  return `https://banking.dkb.de/api/documentstorage/documents/${encodeURIComponent(id)}`;
}

// JSON:API entscheidet ueber den Accept-Header, ob dieselbe URL Metadaten
// oder den Inhalt liefert.
export const DOC_ACCEPT = 'application/pdf';

const FIELDS = {
  id: ['id'],
  title: [
    'attributes.metadata.subject',
    'attributes.subject',
    'attributes.fileName',
    'attributes.filename',
    'attributes.name',
    'attributes.title',
  ],
  date: [
    'attributes.creationDate',
    'attributes.metadata.statementDate',
    'attributes.documentDate',
    'attributes.createdAt',
  ],
  // Nur bei Wertpapierdokumenten vorhanden.
  depotNumber: ['attributes.metadata.depotNumber'],
  // Konto-/Kartenreferenz, um z. B. zwei Kreditkartenabrechnungen vom
  // selben Tag (unterschiedliche Karten) im Dateinamen unterscheiden zu
  // koennen -- der Titel allein reicht dafuer nicht ("Kreditkarten-
  // abrechnung vom 21. August 2026" ist bei beiden Karten identisch).
  // Erste Runde (Kandidaten geraten) hat nichts getroffen -- per echtem
  // Rohbeispiel (11.09.2026) bestaetigt: In den Metadaten selbst steht
  // nichts Brauchbares (nur "cardId", eine UUID), die maskierte
  // Kartennummer steckt stattdessen im Dateinamen, z. B.
  // "Kreditkarte_4930XXXXXXXX2523_Abrechnung_20260821.pdf". Diese
  // Kandidaten bleiben als Rueckfalloption fuer den Fall, dass die DKB
  // es bei anderen Dokumentarten doch direkt in den Metadaten mitschickt.
  accountRef: [
    'attributes.metadata.iban',
    'attributes.metadata.maskedPan',
    'attributes.metadata.maskedCardNumber',
    'attributes.metadata.cardNumber',
    'attributes.metadata.pan',
    'attributes.metadata.accountNumber',
    'attributes.metadata.kontonummer',
  ],
};

// "Kreditkarte_4930XXXXXXXX2523_Abrechnung_20260821.pdf" -> "2523".
// Die letzten vier Ziffern reichen als Unterscheidung -- genau das, was
// die DKB dem Menschen selbst als Kartenkennung zeigt ("•••• •••• 2523").
const CARD_LAST4_PATTERN = /Kreditkarte_\d{4}X+(\d{4})_/i;

function pluck(obj, dottedPath) {
  if (!dottedPath) return undefined;
  return dottedPath.split('.').reduce((acc, key) => (acc == null ? undefined : acc[key]), obj);
}

function pluckFirst(obj, candidates) {
  if (!candidates) return undefined;
  for (const c of candidates) {
    const v = pluck(obj, c);
    if (v !== undefined && v !== null && v !== '') return v;
  }
  return undefined;
}

/** '15.03.2024' oder ISO -> '2024-03-15'. null, wenn unklar. */
export function normalizeDate(raw) {
  if (!raw) return null;
  const s = String(raw).trim();
  let m = s.match(/(\d{2})\.(\d{2})\.(\d{4})/);
  if (m) return `${m[3]}-${m[2]}-${m[1]}`;
  m = s.match(/(\d{4})-(\d{2})-(\d{2})/);
  if (m) return `${m[1]}-${m[2]}-${m[3]}`;
  return null;
}

function deriveAccountRef(raw) {
  const fromMetadata = pluckFirst(raw, FIELDS.accountRef);
  if (fromMetadata) return fromMetadata;

  const fileName = pluckFirst(raw, ['attributes.fileName', 'attributes.filename']);
  const match = typeof fileName === 'string' ? fileName.match(CARD_LAST4_PATTERN) : null;
  return match ? match[1] : null;
}

/** Ein Roh-Eintrag aus der JSON:API-Liste -> { id, title, date, depotNumber, accountRef }. */
export function extractItem(raw) {
  const id = pluckFirst(raw, FIELDS.id);
  const title = pluckFirst(raw, FIELDS.title) ?? `dokument-${id}`;
  const date = normalizeDate(pluckFirst(raw, FIELDS.date));
  const depotNumber = pluckFirst(raw, FIELDS.depotNumber) ?? null;
  const accountRef = deriveAccountRef(raw);
  return { id, title, date, depotNumber, accountRef };
}
