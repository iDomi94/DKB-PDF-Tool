//! Einstellungen: Ordner-/Dateiname-Vorlagen, Kategorie-Muster,
//! Depot-Besitzer-Zuordnung. Liegen als settings.json im
//! App-Konfigurationsordner (nicht im Repo/Bundle) -- die Standardwerte
//! hier sind bewusst generisch (deutsche Bankdokument-Bezeichnungen,
//! keine Namen oder echten Depotnummern), damit ein oeffentliches Repo
//! keine persoenlichen Daten des Autors enthaelt.

use crate::categorize::{CategoryPattern, DepotOwner};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(rename = "outputDir")]
    pub output_dir: Option<String>,
    #[serde(rename = "pathTemplate")]
    pub path_template: String,
    #[serde(rename = "filenameTemplate")]
    pub filename_template: String,
    #[serde(rename = "categoryPatterns")]
    pub category_patterns: Vec<CategoryPattern>,
    #[serde(rename = "securitiesSubPatterns")]
    pub securities_sub_patterns: Vec<CategoryPattern>,
    #[serde(rename = "depotOwners")]
    pub depot_owners: Vec<DepotOwner>,
}

fn pattern(pattern: &str, label: &str) -> CategoryPattern {
    CategoryPattern {
        pattern: pattern.to_string(),
        label: label.to_string(),
    }
}

fn default_category_patterns() -> Vec<CategoryPattern> {
    vec![
        pattern(r"^Kontoauszug", "Kontoauszuege"),
        pattern(r"^Kreditkartenabrechnung", "Kreditkartenabrechnungen"),
        pattern(
            r"^(Jahressteuerbescheinigung|Erträgnisaufstellung)",
            "Steuerbescheinigungen",
        ),
        pattern(
            r"^(Vereinbarung|Vorvertragliche Informationen|Vertragsentwurf|Zustimmung Eröffnung|Änderungsangebot|Informationsbogen|Legitimation Vollmacht|Bestätigung der Vollmacht)|^Vertrag|vertrag$",
            "Vertragsinformationen",
        ),
        pattern(r"^Mitteilung", "Mitteilungen"),
        pattern(
            // "Wertpapierabrechnung" zusaetzlich zu "Abrechnung Kauf/
            // Verkauf": aeltere, archivierte Dokumente (vor einer
            // DKB-Systemumstellung) verwenden diese abweichende
            // Formulierung -- live gefunden beim Sortieren eines alten
            // Exports (12.09.2026), sonst blieben solche Dateien ganz
            // ohne Kategorie.
            r"^(Abrechnung (Kauf|Verkauf)|Wertpapierabrechnung|Kosteninformation|Ertragsabrechnung|Vorabpauschale|Depotauszug|Kap(i)?talma(ß|ss)nahme|Hauptversammlung|Auftragsbestätigung|PRIIP-Verordnung)",
            "Wertpapierdokumente",
        ),
    ]
}

fn default_securities_sub_patterns() -> Vec<CategoryPattern> {
    vec![
        pattern(r"^Abrechnung Kauf", "Kauf"),
        pattern(r"^Abrechnung Verkauf", "Verkauf"),
        // Aeltere Titel-Konvention: eigenstaendiges "Kauf"/"Verkauf" vor
        // einer WKN-Angabe statt "Abrechnung Kauf/Verkauf" -- z. B.
        // "Kauf - WKN A0RPWH - Wertpapierabrechnung vom ...". Wortgrenzen
        // (\b), damit "Kauf" nicht versehentlich in "Verkauf" mit-matcht.
        pattern(r"\bVerkauf\b", "Verkauf"),
        pattern(r"\bKauf\b", "Kauf"),
        pattern(r"^(Kosteninformation|PRIIP-Verordnung)", "Informationen"),
        // "Vorabpauschale Investmentfonds" ist der tatsaechliche
        // PDF-Titel; die API nennt dasselbe Dokument "Ertragsabrechnung"
        // im "subject"-Feld -- beide Formulierungen kommen vor
        // (bestaetigt beim Testen des Sortieren-Nebentools, 11.09.2026:
        // ein per API als "Ertragsabrechnung" bezeichnetes Dokument
        // enthaelt das Wort im gedruckten PDF gar nicht, dafuer aber
        // "Vorabpauschale").
        pattern(r"^(Ertragsabrechnung|Vorabpauschale)", "Ertrag"),
        pattern(r"^Depotauszug", "Depotauszuege"),
        pattern(r"^Kap(i)?talma(ß|ss)nahme", "Kapitalmassnahmen"),
        pattern(r"^Hauptversammlung", "Hauptversammlung"),
        pattern(r"^Auftragsbestätigung", "Auftragsbestaetigungen"),
    ]
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            output_dir: None,
            path_template: "{year}/{category}/{owner}/{subCategory}".to_string(),
            filename_template: "{date}_{title}".to_string(),
            category_patterns: default_category_patterns(),
            securities_sub_patterns: default_securities_sub_patterns(),
            depot_owners: Vec::new(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Konfigurationsordner nicht auffindbar: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Konfigurationsordner nicht anlegbar: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("settings.json nicht lesbar: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("settings.json ist kein gueltiges JSON: {e}"))
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Einstellungen nicht serialisierbar: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("settings.json nicht schreibbar: {e}"))
}
