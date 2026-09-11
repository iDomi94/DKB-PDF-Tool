//! Gedaechtnis ueber Laeufe hinweg: welche Dokumente wurden schon
//! heruntergeladen. Liegt als "_manifest.json" im Ausgabeordner. Ohne
//! das wuerde jeder Lauf wieder alle Dokumente neu holen.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ManifestEntry {
    pub key: String,
    pub title: String,
    pub date: Option<String>,
    pub file: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: u64,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Manifest {
    #[serde(default)]
    pub documents: Vec<ManifestEntry>,
}

impl Manifest {
    pub fn has(&self, key: &str) -> bool {
        self.documents.iter().any(|d| d.key == key)
    }

    pub fn record(&mut self, entry: ManifestEntry) {
        if let Some(existing) = self.documents.iter_mut().find(|d| d.key == entry.key) {
            *existing = entry;
        } else {
            self.documents.push(entry);
        }
    }
}

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load(path: &Path) -> Result<Manifest, String> {
    if !path.exists() {
        return Ok(Manifest::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("Manifest nicht lesbar: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Manifest ist kein gueltiges JSON: {e}"))
}

pub fn save(path: &Path, manifest: &Manifest) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Manifest nicht serialisierbar: {e}"))?;
    fs::write(path, raw).map_err(|e| format!("Manifest nicht schreibbar: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_and_record() {
        let mut m = Manifest::default();
        assert!(!m.has("abc"));
        m.record(ManifestEntry {
            key: "abc".into(),
            title: "Test".into(),
            date: Some("2026-01-01".into()),
            file: "2026/Test.pdf".into(),
            fetched_at: 0,
        });
        assert!(m.has("abc"));
        assert_eq!(m.documents.len(), 1);
    }

    #[test]
    fn record_overwrites_existing_key() {
        let mut m = Manifest::default();
        m.record(ManifestEntry {
            key: "abc".into(),
            title: "Alt".into(),
            date: None,
            file: "a.pdf".into(),
            fetched_at: 1,
        });
        m.record(ManifestEntry {
            key: "abc".into(),
            title: "Neu".into(),
            date: None,
            file: "b.pdf".into(),
            fetched_at: 2,
        });
        assert_eq!(m.documents.len(), 1);
        assert_eq!(m.documents[0].title, "Neu");
    }

    #[test]
    fn roundtrip_through_disk() {
        let dir = std::env::temp_dir().join(format!("dkb-manifest-test-{}", now_epoch_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_manifest.json");

        let mut m = Manifest::default();
        m.record(ManifestEntry {
            key: "x".into(),
            title: "T".into(),
            date: None,
            file: "x.pdf".into(),
            fetched_at: 42,
        });
        save(&path, &m).unwrap();

        let loaded = load(&path).unwrap();
        assert!(loaded.has("x"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
