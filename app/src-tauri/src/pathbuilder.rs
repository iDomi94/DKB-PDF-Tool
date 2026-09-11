//! Vorlagen-Engine fuer Ordnerstruktur und Dateiname. Platzhalter wie
//! "{year}" werden ersetzt; ein Pfadsegment, das nach dem Ersetzen leer
//! ist (z. B. "{owner}" bei einem Dokument ohne Depotbezug), faellt
//! komplett weg -- so bleibt z. B. "{year}/{category}/{owner}" bei
//! Kontoauszuegen "2026/Kontoauszuege" statt einem leeren Owner-Ordner.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn render_template(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        let mut key = String::new();
        let mut closed = false;
        while let Some(c2) = chars.next() {
            if c2 == '}' {
                closed = true;
                break;
            }
            key.push(c2);
        }
        if closed {
            if let Some(v) = vars.get(key.as_str()) {
                out.push_str(v);
            }
            // Unbekannter Platzhalter -> stillschweigend leer, kein Fehler:
            // die Vorlage kommt vom Nutzer, ein Tippfehler soll nicht den
            // ganzen Lauf abbrechen.
        } else {
            out.push('{');
            out.push_str(&key);
        }
    }
    out
}

/// Zerlegt die Pfad-Vorlage an "/", ersetzt Platzhalter pro Segment und
/// laesst leer gewordene Segmente weg.
pub fn render_path_segments(template: &str, vars: &HashMap<&str, String>) -> Vec<String> {
    template
        .split('/')
        .map(|seg| render_template(seg, vars))
        .filter(|seg| !seg.trim().is_empty())
        .collect()
}

/// Haengt "_1", "_2", ... vor die Endung, bis ein freier Pfad gefunden
/// ist. Fuer die "Beide behalten"-Option im Duplikat-Dialog.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("datei");
    let ext = path.extension().and_then(|s| s.to_str());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut n: u32 = 1;
    loop {
        let name = match ext {
            Some(ext) => format!("{stem}_{n}.{ext}"),
            None => format!("{stem}_{n}"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn substitutes_known_placeholders() {
        let v = vars(&[("year", "2026"), ("category", "Kontoauszuege")]);
        assert_eq!(
            render_template("{year}/{category}", &v),
            "2026/Kontoauszuege"
        );
    }

    #[test]
    fn drops_empty_segments() {
        let v = vars(&[
            ("year", "2026"),
            ("category", "Kontoauszuege"),
            ("owner", ""),
        ]);
        assert_eq!(
            render_path_segments("{year}/{category}/{owner}", &v),
            vec!["2026".to_string(), "Kontoauszuege".to_string()]
        );
    }

    #[test]
    fn keeps_owner_segment_when_present() {
        let v = vars(&[
            ("year", "2026"),
            ("category", "Wertpapierdokumente"),
            ("owner", "Testperson"),
            ("subCategory", "Kauf"),
        ]);
        assert_eq!(
            render_path_segments("{year}/{category}/{owner}/{subCategory}", &v),
            vec!["2026", "Wertpapierdokumente", "Testperson", "Kauf"]
        );
    }

    #[test]
    fn unknown_placeholder_becomes_empty_not_error() {
        let v = vars(&[("year", "2026")]);
        assert_eq!(render_template("{year}/{nichtVorhanden}", &v), "2026/");
    }

    #[test]
    fn unique_path_returns_original_when_free() {
        let dir = std::env::temp_dir().join("dkb-pathbuilder-test-free");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let candidate = dir.join("datei.pdf");
        assert_eq!(unique_path(&candidate), candidate);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unique_path_bumps_counter_until_free() {
        let dir = std::env::temp_dir().join("dkb-pathbuilder-test-bump");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("datei.pdf");
        std::fs::write(&base, b"x").unwrap();
        std::fs::write(dir.join("datei_1.pdf"), b"x").unwrap();
        assert_eq!(unique_path(&base), dir.join("datei_2.pdf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
