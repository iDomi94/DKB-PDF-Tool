// Sidecar-Anbindung fuer den Online-Abruf (M1) + Einstellungen/
// Kategorisierung/Vorlagen (M2) + Download-Orchestrierung mit
// Duplikat-Dialog (M3).
//
// Bewusst kein tauri-plugin-shell fuer den Sidecar: die Berechtigungs-
// schicht des Plugins gilt fuer von JS direkt aufrufbare Befehle. Wir
// rufen den Sidecar stattdessen aus einer eigenen #[tauri::command]-
// Funktion in Rust auf (per tokio::process), das braucht keine
// zusaetzlichen Capability-Eintraege. Das Plugin kommt erst bei M5
// wieder dazu, wenn der Sidecar als gebuendelte externe Binary (nicht
// mehr per System-Node) aufgerufen wird.

mod categorize;
mod manifest;
mod pathbuilder;
mod settings;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use settings::Settings;

// ---------------------------------------------------------------------------
// Sidecar-Prozess: Start, stdin-Zugriff, Ereignis-Kanal.
// ---------------------------------------------------------------------------

/// `events` haelt eingehende Sidecar-Zeilen zusaetzlich zur normalen
/// Weiterleitung ins UI vor -- damit Befehle wie "list"/"fetch" gezielt
/// auf ihre Antwort warten koennen (siehe `wait_for_any`), waehrend das
/// Roh-Log im Online-Abruf-Tab trotzdem alles sieht.
struct SidecarHandle {
    stdin: Mutex<Option<ChildStdin>>,
    events: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

/// In einer gebauten/installierten App liegt der Sidecar im
/// Ressourcen-Verzeichnis (siehe "bundle.resources" in tauri.conf.json --
/// sidecar/src + sidecar/node_modules werden beim Bauen dorthin kopiert).
/// Im Dev-Modus (`tauri dev`) existiert dieses Verzeichnis nicht, dort
/// wird direkt aus dem Projektordner gelesen. Ohne diesen Fallback wuerde
/// eine echte, bei einem Nutzer installierte App den Sidecar nicht
/// finden -- der alte, rein Dev-taugliche Pfad ueber CARGO_MANIFEST_DIR
/// zeigt auf den Rechner, auf dem gebaut wurde, nicht auf den des Nutzers.
fn sidecar_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("sidecar/src/main.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sidecar/src/main.mjs");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err("Sidecar-Skript weder im Ressourcen- noch im Dev-Verzeichnis gefunden".to_string())
}

/// Portable Node.js-Laufzeit, die CI beim Bauen unter
/// "sidecar/node-runtime/<os>-<arch>/" mitbuendelt (siehe
/// .github/workflows/release.yml) -- damit muessen Nutzer kein eigenes
/// Node.js installieren, nur Google Chrome fuer die Anmeldung. Im
/// Dev-Modus gibt es diesen Ordner nicht; dort faellt `ensure_started`
/// automatisch auf das System-`node` zurueck (muss bei der Entwicklung
/// selbst installiert sein).
fn bundled_node_path(app: &AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;

    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    };
    // Bewusst nur die beiden praktisch relevanten Architekturen -- der
    // macOS-Build ist "universal" (beide Slices in einer Bin-Datei), zur
    // Laufzeit meldet env::consts::ARCH trotzdem korrekt die tatsaechlich
    // ausgefuehrte Architektur (macOS waehlt den passenden Slice schon
    // beim Programmstart, nicht erst hier).
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    let bin_name = if cfg!(windows) { "node.exe" } else { "node" };

    let candidate = resource_dir.join(format!("node-runtime/{os}-{arch}/{bin_name}"));
    candidate.exists().then_some(candidate)
}

async fn ensure_started(app: &AppHandle, handle: &SidecarHandle) -> Result<(), String> {
    let mut stdin_guard = handle.stdin.lock().await;
    if stdin_guard.is_some() {
        return Ok(());
    }

    let script = sidecar_script_path(app)?;
    if !script.exists() {
        return Err(format!(
            "Sidecar-Skript nicht gefunden: {}",
            script.display()
        ));
    }

    let node_bin: std::ffi::OsString = bundled_node_path(app)
        .map(std::ffi::OsString::from)
        .unwrap_or_else(|| "node".into());

    let mut child = Command::new(&node_bin)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            format!(
                "Sidecar konnte nicht gestartet werden: {e}. \
                 In der fertig gebauten App wird eine mitgelieferte Node.js-Laufzeit \
                 erwartet; im Entwicklungsmodus muss stattdessen Node.js selbst \
                 installiert sein (node.js.org)."
            )
        })?;

    let stdin = child.stdin.take().expect("stdin war als piped angefordert");
    let stdout = child
        .stdout
        .take()
        .expect("stdout war als piped angefordert");

    let (tx, rx) = mpsc::unbounded_channel::<String>();

    *stdin_guard = Some(stdin);
    drop(stdin_guard);
    *handle.events.lock().await = Some(rx);

    let app_handle = app.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.trim().is_empty() {
                        let _ = app_handle.emit("sidecar-event", &line);
                        let _ = tx.send(line);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let line =
                        format!("{{\"type\":\"error\",\"message\":\"stdout-Lesefehler: {err}\"}}");
                    let _ = app_handle.emit("sidecar-event", &line);
                    let _ = tx.send(line);
                    break;
                }
            }
        }
        let _ = child.wait().await;
        let _ = app_handle.emit("sidecar-exited", ());

        if let Some(handle) = app_handle.try_state::<SidecarHandle>() {
            *handle.stdin.lock().await = None;
            *handle.events.lock().await = None;
        }
    });

    Ok(())
}

async fn send_command(handle: &SidecarHandle, payload: &Value) -> Result<(), String> {
    let mut guard = handle.stdin.lock().await;
    let stdin = guard.as_mut().ok_or("Sidecar ist nicht (mehr) aktiv")?;
    let line = format!("{payload}\n");
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("Konnte Kommando nicht senden: {e}"))
}

/// Wartet auf die naechste Sidecar-Zeile, deren `"type"` in
/// `expected_types` vorkommt. Ein `"type":"error"` wird dabei immer als
/// Abbruch behandelt, egal worauf gerade gewartet wird -- ein
/// Sidecar-weiter Fehler (z. B. "nicht angemeldet") soll nicht bis zum
/// Timeout uebersehen werden. Andere, nicht passende Ereignisse (status,
/// ready, ...) werden einfach uebersprungen; sie sind schon per
/// "sidecar-event" ins UI geflossen.
async fn wait_for_any(
    events: &mut mpsc::UnboundedReceiver<String>,
    expected_types: &[&str],
    timeout: Duration,
) -> Result<Value, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "Zeitüberschreitung beim Warten auf {expected_types:?}"
            ));
        }
        let line = match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(line)) => line,
            Ok(None) => return Err("Sidecar-Verbindung beendet".into()),
            Err(_) => {
                return Err(format!(
                    "Zeitüberschreitung beim Warten auf {expected_types:?}"
                ))
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let t = value.get("type").and_then(Value::as_str).unwrap_or("");
        if t == "error" {
            let msg = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unbekannter Fehler");
            return Err(msg.to_string());
        }
        if expected_types.contains(&t) {
            return Ok(value);
        }
    }
}

#[tauri::command]
async fn sidecar_send(
    app: AppHandle,
    handle: tauri::State<'_, SidecarHandle>,
    cmd: String,
) -> Result<(), String> {
    ensure_started(&app, &handle).await?;
    send_command(&handle, &serde_json::json!({ "cmd": cmd })).await
}

// ---------------------------------------------------------------------------
// Einstellungen (M2)
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings)
}

#[tauri::command]
fn default_settings() -> Settings {
    Settings::default()
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    rx.await
        .ok()
        .flatten()
        .and_then(|f| f.into_path().ok())
        .map(|p| p.display().to_string())
}

// ---------------------------------------------------------------------------
// Live-Vorschau fuer den Muster-/Vorlagen-Editor (M2)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SampleDoc {
    #[serde(default)]
    id: String,
    title: String,
    date: String,
    #[serde(rename = "depotNumber")]
    depot_number: Option<String>,
    #[serde(rename = "accountRef")]
    account_ref: Option<String>,
}

#[derive(Serialize)]
struct PreviewResult {
    category: Option<String>,
    #[serde(rename = "subCategory")]
    sub_category: Option<String>,
    owner: Option<String>,
    #[serde(rename = "pathSegments")]
    path_segments: Vec<String>,
    filename: String,
}

#[tauri::command]
fn preview_document(settings: Settings, sample: SampleDoc) -> PreviewResult {
    let r = categorize::resolve_document(
        &categorize::DocumentInput {
            id: &sample.id,
            title: &sample.title,
            date: &sample.date,
            depot_number: sample.depot_number.as_deref(),
            account_ref: sample.account_ref.as_deref(),
            category_override: None,
            sub_category_override: None,
        },
        &settings,
    );
    PreviewResult {
        category: r.category,
        sub_category: r.sub_category,
        owner: r.owner,
        path_segments: r.path_segments,
        filename: r.filename,
    }
}

// ---------------------------------------------------------------------------
// Download-Orchestrierung (M3)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct ListedItem {
    id: String,
    title: String,
    date: Option<String>,
    #[serde(rename = "depotNumber")]
    depot_number: Option<String>,
    #[serde(rename = "accountRef")]
    account_ref: Option<String>,
}

#[derive(Deserialize)]
enum DuplicateAction {
    Skip,
    Overwrite,
    KeepBoth,
}

/// Haelt den "Weitermach-Knopf" fuer eine offene Duplikat-Frage. Der
/// Download-Lauf pausiert per oneshot-Channel, bis das Frontend per
/// `resolve_duplicate` antwortet.
struct DuplicateWait {
    sender: Mutex<Option<oneshot::Sender<DuplicateAction>>>,
}

#[tauri::command]
async fn resolve_duplicate(
    state: tauri::State<'_, DuplicateWait>,
    action: DuplicateAction,
) -> Result<(), String> {
    let sender = state.sender.lock().await.take();
    match sender {
        Some(tx) => {
            let _ = tx.send(action);
            Ok(())
        }
        None => Err("Keine offene Duplikat-Frage".into()),
    }
}

/// Pause/Abbrechen fuer einen laufenden Download. `notify` weckt eine
/// wartende Pause auf (bei "fortsetzen" oder "abbrechen"), ohne Polling.
struct DownloadControl {
    paused: std::sync::atomic::AtomicBool,
    cancelled: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
}

impl DownloadControl {
    fn reset(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.cancelled
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Blockiert, solange pausiert ist. Kehrt sofort zurueck, sobald
    /// entweder fortgesetzt oder abgebrochen wird.
    async fn wait_while_paused(&self) {
        while self.is_paused() && !self.is_cancelled() {
            self.notify.notified().await;
        }
    }

    /// Wartet, bis abgebrochen wird -- fuer `tokio::select!` gegen einen
    /// laufenden Sidecar-Aufruf, damit "Abbrechen" auch mitten im Warten
    /// auf eine Antwort sofort wirkt statt erst danach.
    async fn until_cancelled(&self) {
        while !self.is_cancelled() {
            self.notify.notified().await;
        }
    }
}

#[tauri::command]
async fn pause_download(control: tauri::State<'_, DownloadControl>) -> Result<(), String> {
    control
        .paused
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn resume_download(control: tauri::State<'_, DownloadControl>) -> Result<(), String> {
    control
        .paused
        .store(false, std::sync::atomic::Ordering::SeqCst);
    control.notify.notify_waiters();
    Ok(())
}

#[tauri::command]
async fn cancel_download(
    control: tauri::State<'_, DownloadControl>,
    dup_wait: tauri::State<'_, DuplicateWait>,
) -> Result<(), String> {
    control
        .cancelled
        .store(true, std::sync::atomic::Ordering::SeqCst);
    control
        .paused
        .store(false, std::sync::atomic::Ordering::SeqCst);
    control.notify.notify_waiters();

    // Haengt der Lauf gerade an einer offenen Duplikat-Frage, mit "Skip"
    // befreien -- sonst wuerde "Abbrechen" erst nach einer Antwort greifen.
    if let Some(tx) = dup_wait.sender.lock().await.take() {
        let _ = tx.send(DuplicateAction::Skip);
    }
    Ok(())
}

const MAX_CONSECUTIVE_FAILURES: u32 = 5;

#[tauri::command]
async fn start_download(
    app: AppHandle,
    handle: tauri::State<'_, SidecarHandle>,
    dup_wait: tauri::State<'_, DuplicateWait>,
    control: tauri::State<'_, DownloadControl>,
) -> Result<(), String> {
    // Zustand von einem etwaigen vorherigen Lauf nicht mitschleppen --
    // sonst wuerde ein neuer Lauf sofort als "abgebrochen" gelten, wenn
    // der vorherige per "Abbrechen" beendet wurde.
    control.reset();

    // Bewusst die gespeicherten Einstellungen laden, nicht ungespeicherte
    // Formularwerte vom Frontend uebernehmen -- der Download soll immer
    // mit dem Stand arbeiten, der auch tatsaechlich gesichert ist.
    let settings = settings::load(&app)?;
    ensure_started(&app, &handle).await?;

    let output_dir = settings
        .output_dir
        .clone()
        .ok_or("Kein Ausgabeordner gewählt (siehe Einstellungen)")?;
    let output_dir = PathBuf::from(output_dir);
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Ausgabeordner nicht anlegbar: {e}"))?;

    send_command(&handle, &serde_json::json!({ "cmd": "list" })).await?;

    let mut events_guard = handle.events.lock().await;
    let events = events_guard.as_mut().ok_or("Sidecar-Ereigniskanal fehlt")?;

    let list_event = wait_for_any(events, &["list_result"], Duration::from_secs(180)).await?;
    let items: Vec<ListedItem> = serde_json::from_value(
        list_event
            .get("items")
            .cloned()
            .unwrap_or(Value::Array(vec![])),
    )
    .map_err(|e| format!("Ungültige Listenantwort: {e}"))?;

    let manifest_path = output_dir.join("_manifest.json");
    let mut doc_manifest = manifest::load(&manifest_path)?;
    let pending: Vec<ListedItem> = items
        .into_iter()
        .filter(|i| !doc_manifest.has(&i.id))
        .collect();

    let total = pending.len();
    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "phase": "pending", "done": 0, "total": total }),
    );

    let mut done = 0usize;
    let mut consecutive_failures = 0u32;

    for item in pending {
        if control.is_paused() {
            let _ = app.emit(
                "download-progress",
                serde_json::json!({ "phase": "paused" }),
            );
            control.wait_while_paused().await;
        }
        if control.is_cancelled() {
            let _ = app.emit(
                "download-progress",
                serde_json::json!({ "phase": "cancelled", "done": done, "total": total }),
            );
            return Ok(());
        }

        send_command(
            &handle,
            &serde_json::json!({ "cmd": "fetch", "id": item.id }),
        )
        .await?;

        let doc_event = tokio::select! {
            result = wait_for_any(events, &["document", "document_error"], Duration::from_secs(60)) => result,
            _ = control.until_cancelled() => Err("Abgebrochen".to_string()),
        };

        let outcome = match doc_event {
            Ok(value) if value.get("type").and_then(Value::as_str) == Some("document") => {
                write_document(&app, &output_dir, &settings, &item, &value, &dup_wait).await
            }
            Ok(value) => {
                let msg = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Unbekannter Fehler beim Laden")
                    .to_string();
                Err(msg)
            }
            Err(e) => Err(e),
        };

        // "Abbrechen" waehrend einer offenen Duplikat-Frage schickt intern
        // ein "Skip", um den Wartepunkt zu loesen (siehe cancel_download)
        // -- das darf aber nicht als echtes Uebersprungen im Manifest
        // landen. Deshalb hier zuerst auf den Abbruch pruefen, bevor das
        // Ergebnis normal verarbeitet wird.
        if control.is_cancelled() {
            let _ = app.emit(
                "download-progress",
                serde_json::json!({ "phase": "cancelled", "done": done, "total": total }),
            );
            return Ok(());
        }

        match outcome {
            Ok(Some(entry)) => {
                doc_manifest.record(entry);
                manifest::save(&manifest_path, &doc_manifest)?;
                done += 1;
                consecutive_failures = 0;
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({ "phase": "progress", "done": done, "total": total, "title": item.title }),
                );
            }
            Ok(None) => {
                // Uebersprungen (Duplikat -> "Skip"). Zaehlt nicht als
                // Fehler, aber auch nicht als frisch geladen; im Manifest
                // trotzdem vermerken, damit kuenftige Laeufe es nicht
                // wieder anbieten.
                doc_manifest.record(manifest::ManifestEntry {
                    key: item.id.clone(),
                    title: item.title.clone(),
                    date: item.date.clone(),
                    file: String::new(),
                    fetched_at: manifest::now_epoch_secs(),
                });
                manifest::save(&manifest_path, &doc_manifest)?;
                consecutive_failures = 0;
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({ "phase": "skipped", "done": done, "total": total, "title": item.title }),
                );
            }
            Err(message) => {
                consecutive_failures += 1;
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({ "phase": "item_error", "title": item.title, "message": message }),
                );
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    let _ = app.emit(
                        "download-progress",
                        serde_json::json!({
                            "phase": "aborted",
                            "message": format!("{MAX_CONSECUTIVE_FAILURES} Fehler in Folge -- Lauf abgebrochen.")
                        }),
                    );
                    return Err(format!(
                        "{MAX_CONSECUTIVE_FAILURES} Fehler in Folge -- Lauf abgebrochen."
                    ));
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "phase": "done", "done": done, "total": total }),
    );
    Ok(())
}

/// Schreibt ein einzelnes erfolgreich geladenes Dokument. `Ok(Some(entry))`
/// bei erfolgreichem Schreiben, `Ok(None)` wenn der Nutzer ein Duplikat
/// uebersprungen hat, `Err(..)` bei einem echten Fehler.
async fn write_document(
    app: &AppHandle,
    output_dir: &std::path::Path,
    settings: &Settings,
    item: &ListedItem,
    doc_event: &Value,
    dup_wait: &DuplicateWait,
) -> Result<Option<manifest::ManifestEntry>, String> {
    use base64::Engine;

    let content_b64 = doc_event
        .get("contentBase64")
        .and_then(Value::as_str)
        .ok_or("Sidecar-Antwort ohne Inhalt")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_b64)
        .map_err(|e| format!("Ungültige Base64-Antwort: {e}"))?;

    let resolved = categorize::resolve_document(
        &categorize::DocumentInput {
            id: &item.id,
            title: &item.title,
            date: item.date.as_deref().unwrap_or(""),
            depot_number: item.depot_number.as_deref(),
            account_ref: item.account_ref.as_deref(),
            category_override: None,
            sub_category_override: None,
        },
        settings,
    );

    let mut target = output_dir.to_path_buf();
    for seg in &resolved.path_segments {
        target.push(seg);
    }
    target.push(&resolved.filename);

    let Some(target) = resolve_write_target(app, dup_wait, target, &bytes, &item.title).await?
    else {
        return Ok(None);
    };

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Ordner nicht anlegbar: {e}"))?;
    }
    std::fs::write(&target, &bytes).map_err(|e| format!("Datei nicht schreibbar: {e}"))?;

    let file_rel = target
        .strip_prefix(output_dir)
        .unwrap_or(&target)
        .display()
        .to_string();

    Ok(Some(manifest::ManifestEntry {
        key: item.id.clone(),
        title: item.title.clone(),
        date: item.date.clone(),
        file: file_rel,
        fetched_at: manifest::now_epoch_secs(),
    }))
}

/// Prueft, ob `target` schon existiert, fragt bei Bedarf per Duplikat-
/// Dialog nach (Skip/Overwrite/KeepBoth) und gibt den tatsaechlich zu
/// schreibenden Pfad zurueck -- `None` bei "Skip". Gemeinsam genutzt vom
/// Online-Abruf (`write_document`) und dem Sortieren-Nebentool
/// (`sort_one_file`).
async fn resolve_write_target(
    app: &AppHandle,
    dup_wait: &DuplicateWait,
    target: PathBuf,
    new_bytes: &[u8],
    title: &str,
) -> Result<Option<PathBuf>, String> {
    if !target.exists() {
        return Ok(Some(target));
    }

    let existing = std::fs::read(&target).unwrap_or_default();
    let identical = existing == new_bytes;

    let (tx, rx) = oneshot::channel();
    *dup_wait.sender.lock().await = Some(tx);
    let _ = app.emit(
        "duplicate-found",
        serde_json::json!({
            "path": target.display().to_string(),
            "existingSize": existing.len(),
            "newSize": new_bytes.len(),
            "identical": identical,
            "title": title,
        }),
    );

    let action = rx
        .await
        .map_err(|_| "Duplikat-Dialog wurde ohne Antwort geschlossen".to_string())?;

    match action {
        DuplicateAction::Skip => Ok(None),
        DuplicateAction::KeepBoth => Ok(Some(pathbuilder::unique_path(&target))),
        DuplicateAction::Overwrite => Ok(Some(target)),
    }
}

// ---------------------------------------------------------------------------
// Sortieren-Nebentool (M4)
// ---------------------------------------------------------------------------
//
// Nimmt einen Ordner mit bereits vorhandenen PDFs (z.B. aus einem
// frueheren Export, nicht ueber den eigenen Online-Abruf geladen) und
// sortiert sie mit derselben Kategorisierungs-/Duplikat-Logik wie M3 in
// den konfigurierten Ausgabeordner ein. Kopiert (loescht nichts im
// Quellordner) -- der Nutzer kann die Originale nach Pruefung selbst
// entfernen.
//
// Titel-Ermittlung: zuerst der Dateiname (schnell, funktioniert fuer die
// meisten Faelle, da unsere eigenen Downloads den Betreff im Namen
// tragen). Nur wenn das zu keinem Kategorie-Treffer fuehrt, wird der
// PDF-Text extrahiert und -- mit ungeankerten Mustern, siehe
// `match_pattern_in_text` -- erneut versucht. Datum/Depotnummer/
// Kartenreferenz werden nach demselben Schema aus Dateiname bzw.
// PDF-Text geraten (siehe categorize::extract_*).

fn collect_pdfs(dir: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Ordner nicht lesbar: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Verzeichniseintrag nicht lesbar: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_pdfs(&path)?);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
    Ok(out)
}

/// Ergebnis fuer eine Datei: `Ok(Some(()))` geschrieben, `Ok(None)`
/// uebersprungen (Duplikat -> Skip), `Err(..)` echter Fehler.
async fn sort_one_file(
    app: &AppHandle,
    output_dir: &std::path::Path,
    settings: &Settings,
    source: &std::path::Path,
    dup_wait: &DuplicateWait,
) -> Result<Option<()>, String> {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dokument")
        .to_string();
    let title_candidate = stem.replace('_', " ");

    let mut category = categorize::match_pattern(&title_candidate, &settings.category_patterns)
        .map(str::to_string);
    let mut sub_category = None;
    let mut date = categorize::extract_date(&title_candidate);
    let mut depot = categorize::extract_depot_number(&title_candidate);
    let mut account = categorize::extract_card_last4(&stem);

    if category.is_none() {
        // Dateiname allein hat zu keinem Treffer gefuehrt -- Text
        // extrahieren und mit ungeankerten Mustern erneut versuchen.
        if let Ok(text) = pdf_extract::extract_text(source) {
            category = categorize::match_pattern_in_text(&text, &settings.category_patterns)
                .map(str::to_string);
            if date.is_none() {
                date = categorize::extract_date(&text);
            }
            if depot.is_none() {
                depot = categorize::extract_depot_number(&text);
            }
            if account.is_none() {
                account = categorize::extract_card_last4(&text);
            }
            if depot.is_some() {
                sub_category =
                    categorize::match_pattern_in_text(&text, &settings.securities_sub_patterns)
                        .map(str::to_string);
            }
        }
    }
    if depot.is_some() && sub_category.is_none() {
        sub_category =
            categorize::match_pattern(&title_candidate, &settings.securities_sub_patterns)
                .map(str::to_string);
    }

    let bytes = std::fs::read(source).map_err(|e| format!("Datei nicht lesbar: {e}"))?;

    let resolved = categorize::resolve_document(
        &categorize::DocumentInput {
            id: &stem,
            title: &title_candidate,
            date: date.as_deref().unwrap_or(""),
            depot_number: depot.as_deref(),
            account_ref: account.as_deref(),
            category_override: category.as_deref(),
            sub_category_override: sub_category.as_deref(),
        },
        settings,
    );

    let mut target = output_dir.to_path_buf();
    for seg in &resolved.path_segments {
        target.push(seg);
    }
    target.push(&resolved.filename);

    let Some(target) =
        resolve_write_target(app, dup_wait, target, &bytes, &title_candidate).await?
    else {
        return Ok(None);
    };

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Ordner nicht anlegbar: {e}"))?;
    }
    std::fs::copy(source, &target).map_err(|e| format!("Datei nicht kopierbar: {e}"))?;

    Ok(Some(()))
}

#[tauri::command]
async fn sort_folder(
    app: AppHandle,
    dup_wait: tauri::State<'_, DuplicateWait>,
    control: tauri::State<'_, DownloadControl>,
    input_dir: String,
) -> Result<(), String> {
    control.reset();

    let settings = settings::load(&app)?;
    let output_dir = settings
        .output_dir
        .clone()
        .ok_or("Kein Ausgabeordner gewählt (siehe Einstellungen)")?;
    let output_dir = PathBuf::from(output_dir);
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Ausgabeordner nicht anlegbar: {e}"))?;

    let files = collect_pdfs(&PathBuf::from(input_dir))?;
    let total = files.len();
    let _ = app.emit(
        "sort-progress",
        serde_json::json!({ "phase": "pending", "done": 0, "total": total }),
    );

    let mut done = 0usize;
    let mut consecutive_failures = 0u32;

    for path in files {
        if control.is_paused() {
            let _ = app.emit("sort-progress", serde_json::json!({ "phase": "paused" }));
            control.wait_while_paused().await;
        }
        if control.is_cancelled() {
            let _ = app.emit(
                "sort-progress",
                serde_json::json!({ "phase": "cancelled", "done": done, "total": total }),
            );
            return Ok(());
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Datei")
            .to_string();

        let outcome = sort_one_file(&app, &output_dir, &settings, &path, &dup_wait).await;

        if control.is_cancelled() {
            let _ = app.emit(
                "sort-progress",
                serde_json::json!({ "phase": "cancelled", "done": done, "total": total }),
            );
            return Ok(());
        }

        match outcome {
            Ok(Some(())) => {
                done += 1;
                consecutive_failures = 0;
                let _ = app.emit(
                    "sort-progress",
                    serde_json::json!({ "phase": "progress", "done": done, "total": total, "title": name }),
                );
            }
            Ok(None) => {
                consecutive_failures = 0;
                let _ = app.emit(
                    "sort-progress",
                    serde_json::json!({ "phase": "skipped", "done": done, "total": total, "title": name }),
                );
            }
            Err(message) => {
                consecutive_failures += 1;
                let _ = app.emit(
                    "sort-progress",
                    serde_json::json!({ "phase": "item_error", "title": name, "message": message }),
                );
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    let _ = app.emit(
                        "sort-progress",
                        serde_json::json!({
                            "phase": "aborted",
                            "message": format!("{MAX_CONSECUTIVE_FAILURES} Fehler in Folge -- Lauf abgebrochen.")
                        }),
                    );
                    return Err(format!(
                        "{MAX_CONSECUTIVE_FAILURES} Fehler in Folge -- Lauf abgebrochen."
                    ));
                }
            }
        }
    }

    let _ = app.emit(
        "sort-progress",
        serde_json::json!({ "phase": "done", "done": done, "total": total }),
    );
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(SidecarHandle {
            stdin: Mutex::new(None),
            events: Mutex::new(None),
        })
        .manage(DuplicateWait {
            sender: Mutex::new(None),
        })
        .manage(DownloadControl {
            paused: std::sync::atomic::AtomicBool::new(false),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        })
        .invoke_handler(tauri::generate_handler![
            sidecar_send,
            get_settings,
            save_settings,
            default_settings,
            pick_folder,
            preview_document,
            start_download,
            resolve_duplicate,
            pause_download,
            resume_download,
            cancel_download,
            sort_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
