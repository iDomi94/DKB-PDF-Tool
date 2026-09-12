# DKB-PDF-Tool

Ein eigenständiges Programm für macOS, Windows und Linux, das die Dokumente
aus dem DKB-Postfach (Kontoauszüge, Kreditkartenabrechnungen,
Wertpapierdokumente, Steuerbescheinigungen, Vertragsinformationen,
Mitteilungen) automatisch abruft und nach frei konfigurierbaren Regeln in
Ordnern ablegt.

> **Kein offizielles DKB-Produkt.** Dieses Tool steht in keiner Verbindung
> zur Deutschen Kreditbank AG. Es automatisiert lediglich das, was man auch
> von Hand im Online-Banking tun könnte: sich anmelden und Dokumente aus dem
> eigenen Postfach herunterladen. Automatisierter Zugriff aufs
> Online-Banking ist in den DKB-Nutzungsbedingungen nicht ausdrücklich
> vorgesehen -- Nutzung auf eigene Verantwortung.

## Warum dieses Tool

Der naheliegende Weg über PSD2/XS2A-Schnittstellen (z. B. Enable Banking)
deckt nur Zahlungskonten ab -- weder Depot noch die Dokumente im Postfach.
Der bisher gängige Weg über das Jameica-Plugin `hibiscus.docmanager`
arbeitet per Screen-Scraping und ist entsprechend fragil. Dieses Tool spricht
stattdessen direkt die interne API an, die auch die DKB-Weboberfläche selbst
nutzt.

## Funktionen

- **Online-Abruf**: einmal im Browserfenster anmelden (PIN/TAN, ganz normal),
  danach automatischer Abruf aller neuen Dokumente. Bereits geladene
  Dokumente werden per Manifest erkannt und nicht erneut heruntergeladen.
- **Sortieren**: ein bereits vorhandener Ordner mit PDFs (z. B. ein alter
  Export) wird mit denselben Regeln einsortiert -- unabhängig vom
  Online-Abruf, rein lokal.
- **Frei konfigurierbare Ordnerstruktur und Dateinamen** über Platzhalter
  wie `{year}`, `{category}`, `{owner}`, `{subCategory}`, `{date}`,
  `{title}`, `{account}`, `{id}`.
- **Frei konfigurierbare Kategorisierung** über reguläre Ausdrücke auf dem
  Dokumententitel -- mit sinnvollen Voreinstellungen für die üblichen
  DKB-Dokumentarten.
- **Depot-Zuordnung**: mehrere Depots (z. B. für verschiedene
  Familienmitglieder) lassen sich Namen zuordnen, für eine zusätzliche
  Unterordner-Ebene bei Wertpapierdokumenten.
- **Duplikat-Erkennung**: existiert eine Datei am Zielort schon, fragt das
  Programm nach (Überspringen / Überschreiben / Beide behalten), statt
  stillschweigend etwas zu tun.
- **Pause/Abbrechen** für laufende Abrufe bzw. Sortier-Läufe.

## Voraussetzungen

- **Google Chrome** muss installiert sein. Die Anmeldung läuft über ein
  echtes, sichtbares Chrome-Fenster -- absichtlich, weil Banking-Seiten
  automatisierte "unsichtbare" Browser erkennen und blockieren.

Alles andere bringt das Programm mit, inklusive einer eigenen Node.js-
Laufzeit für den Online-Abruf -- keine separate Installation nötig.

## Installation

Fertige Programme für macOS, Windows und Linux gibt es unter
[Releases](https://github.com/iDomi94/DKB-PDF-Tool/releases).

- **macOS**: `.dmg` herunterladen, App in den Programme-Ordner ziehen. Da das
  Programm nicht mit einem kostenpflichtigen Apple-Entwicklerzertifikat
  signiert ist, meldet macOS beim ersten Start eine Warnung -- über
  Systemeinstellungen → Datenschutz & Sicherheit trotzdem öffnen lassen.
- **Windows**: `.msi` oder `.exe` herunterladen und ausführen. Windows
  SmartScreen kann ebenfalls warnen (aus demselben Grund: kein
  kostenpflichtiges Code-Signing-Zertifikat) -- über "Weitere Informationen"
  → "Trotzdem ausführen" bestätigen.
- **Linux**: `.AppImage` (ausführbar machen und starten) oder `.deb`
  herunterladen.

## Erste Schritte

1. App starten, Tab **Einstellungen** öffnen, Ausgabeordner wählen und
   speichern. Die Standard-Kategorien und -Vorlagen funktionieren ohne
   weitere Anpassung; bei Bedarf lassen sie sich dort anpassen (Live-Vorschau
   zeigt sofort, was eine Änderung bewirkt).
2. Tab **Online-Abruf**: auf "Anmelden" klicken, im sich öffnenden
   Chrome-Fenster ganz normal mit PIN/TAN anmelden.
3. Zurück im Programm: "Dokumente abrufen" klicken. Fortschritt und
   eventuelle Duplikat-Rückfragen erscheinen im Fenster.
4. Für bereits vorhandene PDFs (z. B. ein alter Export): Tab **Sortieren**,
   Quellordner wählen, "Sortieren starten". Die Originale bleiben dabei
   unverändert -- es wird kopiert, nicht verschoben.

## Datenschutz & Sicherheit

- **Keine Zugangsdaten im Programm.** PIN und TAN werden ausschließlich von
  Hand im echten Chrome-Fenster eingegeben, nie vom Programm selbst
  verarbeitet oder gespeichert.
- **Alles bleibt lokal.** Einstellungen liegen im Konfigurationsordner des
  Betriebssystems, Dokumente im selbst gewählten Ausgabeordner. Es gibt
  keine Cloud-Anbindung, keine Telemetrie, keine Analyse-Dienste.
- Das Chrome-Profil für die Bank-Sitzung liegt unter
  `~/.dkb-postfach-app/profile` (bzw. dem plattformspezifischen Äquivalent)
  -- **nicht** in einem Sync-Ordner wie Nextcloud, Dropbox oder OneDrive
  ablegen. Solche Clients schreiben in die offenen Profildateien und bringen
  Chrome zum Absturz.

## Bekannte Einschränkungen

- Unsignierte Installer lösen bei macOS und Windows Sicherheitswarnungen
  aus (siehe [Installation](#installation)) -- Code-Signing-Zertifikate
  kosten Geld und sind für dieses kleine Projekt (noch) nicht vorgesehen.
- Getestet gegen die DKB-API-Struktur vom September 2026. Ändert die DKB
  ihre Postfach-API grundlegend, kann das Tool bis zu einer Anpassung
  nicht mehr funktionieren.

## Entwicklung

Voraussetzung für die Entwicklung (anders als für die fertige App): lokal
installiertes Node.js. Die mitgelieferte Node.js-Laufzeit wird nur beim
Bauen der Installer erzeugt (siehe `scripts/fetch-node-runtime.sh` und
`.github/workflows/release.yml`), im Dev-Modus greift die App auf das
System-`node` zurück.

```bash
cd app
npm install
cd sidecar && npm install && cd ..
npm run tauri dev
```

Aufbau:

```
app/
  src/            Frontend (reines HTML/CSS/JS, kein Framework)
  src-tauri/      Rust-Hauptprozess: Einstellungen, Kategorisierung,
                  Zielpfad-Aufbau, Duplikat-/Pause-Logik, Sortieren
  sidecar/        Node-Prozess für den Online-Abruf (Login, Liste,
                  Dokument-Download) -- reine Fetch-Maschine, entscheidet
                  bewusst nicht selbst über Zielpfade/Kategorien
```

Tests: `cd app/src-tauri && cargo test`

## Lizenz

[MIT](LICENSE)
