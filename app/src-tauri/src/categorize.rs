//! Kategorisierung, Depot-Besitzer-Zuordnung und Dateinamen-Bereinigung.
//! Portiert aus DKB_Dateiabruf/lib/tasks.mjs + lib/store.mjs -- dort
//! gegen die echte DKB-API entwickelt und getestet (siehe HANDOFF.md
//! dort). Einzige Quelle der Wahrheit fuer diese Logik in diesem Projekt;
//! der Sidecar (Node) entscheidet bewusst NICHT mehr selbst darueber.

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CategoryPattern {
    pub pattern: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DepotOwner {
    #[serde(rename = "depotNumber")]
    pub depot_number: String,
    pub name: String,
}

/// Erster Treffer gewinnt, wie beim JS-Original. Ungueltige Regex in
/// einem Muster wird uebersprungen (nicht der ganze Lauf abgebrochen) --
/// Nutzer koennten in der Einstellungen-UI ein kaputtes Muster eintippen,
/// das darf nur dieses eine Muster wirkungslos machen.
pub fn match_pattern<'a>(title: &str, patterns: &'a [CategoryPattern]) -> Option<&'a str> {
    for p in patterns {
        let Ok(re) = RegexBuilder::new(&p.pattern).case_insensitive(true).build() else {
            continue;
        };
        if re.is_match(title) {
            return Some(&p.label);
        }
    }
    None
}

/// Wie `match_pattern`, aber fuer Freitext statt eines Titels -- z. B.
/// aus einer PDF extrahierter Seitentext. Der Unterschied: unsere Muster
/// sind auf den *Titel* zugeschnitten und beginnen deshalb meist mit "^"
/// (Titel-Anfang). PDF-Text faengt aber mit der Adresse an, nicht dem
/// Betreff ("Herr Max Mustermann Musterstr. 1 ..." vor "Kontoauszug ...")
/// -- ein Anker auf Textanfang traefe da nie. Deshalb werden alle "^" vor
/// dem Kompilieren entfernt, damit dieselben Muster irgendwo im Text
/// treffen koennen statt nur am Anfang.
pub fn match_pattern_in_text<'a>(text: &str, patterns: &'a [CategoryPattern]) -> Option<&'a str> {
    for p in patterns {
        let unanchored = p.pattern.replace('^', "");
        let Ok(re) = RegexBuilder::new(&unanchored)
            .case_insensitive(true)
            .build()
        else {
            continue;
        };
        if re.is_match(text) {
            return Some(&p.label);
        }
    }
    None
}

/// "21.08.2026" oder "2026-08-21" irgendwo im Text -> "2026-08-21".
/// Fuer das Sortieren-Werkzeug (M4): dort gibt es kein API-Datum, das
/// Datum muss aus Dateiname oder PDF-Text geraten werden.
pub fn extract_date(text: &str) -> Option<String> {
    if let Ok(re) = RegexBuilder::new(r"(\d{2})\.(\d{2})\.(\d{4})").build() {
        if let Some(c) = re.captures(text) {
            return Some(format!("{}-{}-{}", &c[3], &c[2], &c[1]));
        }
    }
    if let Ok(re) = RegexBuilder::new(r"(\d{4})-(\d{2})-(\d{2})").build() {
        if let Some(c) = re.captures(text) {
            return Some(format!("{}-{}-{}", &c[1], &c[2], &c[3]));
        }
    }
    None
}

/// "... zu Depot 222222222 ..." irgendwo im Text -> "222222222".
pub fn extract_depot_number(text: &str) -> Option<String> {
    RegexBuilder::new(r"Depot\s+(\d{5,12})")
        .build()
        .ok()?
        .captures(text)
        .map(|c| c[1].to_string())
}

/// "Kreditkarte_4930XXXXXXXX2523_..." irgendwo im Text -> "2523". Siehe
/// sidecar/src/dkbApi.mjs (dort dieselbe Notwendigkeit fuer den
/// Online-Abruf: DKB liefert die maskierte Kartennummer nur im
/// Dateinamen, in keinem Metadatenfeld).
pub fn extract_card_last4(text: &str) -> Option<String> {
    RegexBuilder::new(r"Kreditkarte_\d{4}X+(\d{4})_")
        .case_insensitive(true)
        .build()
        .ok()?
        .captures(text)
        .map(|c| c[1].to_string())
}

pub fn owner_for<'a>(depot_number: Option<&str>, owners: &'a [DepotOwner]) -> Option<&'a str> {
    let depot_number = depot_number?;
    owners
        .iter()
        .find(|o| o.depot_number == depot_number)
        .map(|o| o.name.as_str())
}

/// Ergebnis der vollstaendigen Einordnung eines Dokuments: Kategorie,
/// Unterart/Besitzer (nur bei Depotbezug) und der daraus abgeleitete
/// Zielpfad. Einzige Stelle, die das zusammensetzt -- genutzt sowohl von
/// der Live-Vorschau in den Einstellungen als auch vom echten Download.
pub struct ResolvedDocument {
    pub category: Option<String>,
    pub sub_category: Option<String>,
    pub owner: Option<String>,
    pub path_segments: Vec<String>,
    pub filename: String,
}

/// Eingaben fuer `resolve_document`. Als Struct statt vieler einzelner
/// Parameter, weil seit dem Duplikat-Problem bei Kreditkartenabrechnungen
/// (zwei Karten, selber Tag, identischer Titel -> identischer Dateiname)
/// zwei weitere Felder dazukamen: `id` (immer eindeutig) und `account_ref`
/// (Konto-/Kartenreferenz, falls die API sie liefert) als zusaetzliche
/// Platzhalter, mit denen sich solche Kollisionen im Dateiname-Template
/// aufloesen lassen.
pub struct DocumentInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub date: &'a str,
    pub depot_number: Option<&'a str>,
    pub account_ref: Option<&'a str>,
    /// Wenn gesetzt, wird das statt einer eigenen titel-basierten
    /// Musterpruefung verwendet. Fuer M4 (Sortieren-Nebentool): dort wird
    /// bei Bedarf per PDF-Text-Fallback UND ungeankerten Mustern
    /// (`match_pattern_in_text`) geprueft -- eine andere Strategie, als
    /// `resolve_document` sie hier normalerweise selbst anwendet. `None`
    /// (der Normalfall bei Online-Abruf/Vorschau) heisst: ganz normal aus
    /// `title` ableiten, wie bisher.
    pub category_override: Option<&'a str>,
    pub sub_category_override: Option<&'a str>,
}

pub fn resolve_document(
    input: &DocumentInput,
    settings: &crate::settings::Settings,
) -> ResolvedDocument {
    let title = input.title;
    let category = input
        .category_override
        .map(str::to_string)
        .or_else(|| match_pattern(title, &settings.category_patterns).map(str::to_string));

    // Besitzer + Unterart haengen an der Depotnummer, nicht am Namen der
    // Kategorie -- die ist frei umbenennbar. Nur Wertpapier-Dokumente
    // haben ueberhaupt eine Depotnummer in den Metadaten.
    let (owner, sub_category) = match input.depot_number {
        Some(depot) => (
            owner_for(Some(depot), &settings.depot_owners).map(str::to_string),
            input.sub_category_override.map(str::to_string).or_else(|| {
                match_pattern(title, &settings.securities_sub_patterns).map(str::to_string)
            }),
        ),
        None => (None, None),
    };

    let year = input
        .date
        .get(0..4)
        .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("ohne-datum")
        .to_string();

    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    vars.insert("year", year);
    vars.insert("category", category.clone().unwrap_or_default());
    vars.insert("owner", owner.clone().unwrap_or_default());
    vars.insert("subCategory", sub_category.clone().unwrap_or_default());
    vars.insert("date", input.date.to_string());
    vars.insert("title", title.to_string());
    vars.insert("id", input.id.to_string());
    vars.insert("account", input.account_ref.unwrap_or("").to_string());

    let path_segments: Vec<String> =
        crate::pathbuilder::render_path_segments(&settings.path_template, &vars)
            .into_iter()
            .map(|seg| safe_segment(&seg, 80))
            .collect();

    let raw_filename = crate::pathbuilder::render_template(&settings.filename_template, &vars);
    let filename = format!("{}.pdf", safe_segment(&raw_filename, 120));

    ResolvedDocument {
        category,
        sub_category,
        owner,
        path_segments,
        filename,
    }
}

/// Dateisystem-sicherer Name: Umlaute transliterieren (NICHT einfach die
/// Akzente wegwerfen -- sonst wird "Müller" zu "Mller" statt "Mueller"),
/// dann den Rest ueber NFKD + Streichen von Kombinationszeichen, dann
/// alles, was kein Wortzeichen/Punkt/Bindestrich/Leerzeichen ist, durch
/// "_" ersetzen, Mehrfach-"_" zusammenfassen, Endung erhalten.
pub fn safe_segment(value: &str, max_len: usize) -> String {
    let mut s = value
        .replace('ä', "ae")
        .replace('Ä', "Ae")
        .replace('ö', "oe")
        .replace('Ö', "Oe")
        .replace('ü', "ue")
        .replace('Ü', "Ue")
        .replace('ß', "ss");

    s = s.nfkd().filter(|c| !is_combining_mark(*c)).collect();

    let non_word = RegexBuilder::new(r"[^\w.\- ]+").build().unwrap();
    s = non_word.replace_all(&s, "_").into_owned();
    let whitespace = RegexBuilder::new(r"\s+").build().unwrap();
    s = whitespace.replace_all(&s, "_").into_owned();
    let multi_underscore = RegexBuilder::new(r"_+").build().unwrap();
    s = multi_underscore.replace_all(&s, "_").into_owned();
    let trim_edges = RegexBuilder::new(r"^[._]+|[._]+$").build().unwrap();
    s = trim_edges.replace_all(&s, "").into_owned();

    let (stem, ext) = split_extension(&s);
    let truncated: String = stem.chars().take(max_len).collect();
    format!("{truncated}{ext}")
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F)
}

/// Node's `path.extname`: der letzte Punkt, sofern er nicht das erste
/// Zeichen ist (".gitignore" hat keine Endung).
fn split_extension(s: &str) -> (&str, &str) {
    match s.rfind('.') {
        Some(0) => (s, ""),
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_matches_first_hit() {
        let patterns = vec![
            CategoryPattern {
                pattern: r"^Kontoauszug".into(),
                label: "Kontoauszuege".into(),
            },
            CategoryPattern {
                pattern: r"^Kreditkartenabrechnung".into(),
                label: "Kreditkartenabrechnungen".into(),
            },
        ];
        assert_eq!(
            match_pattern("Kontoauszug 9/2026 vom 07.09.2026", &patterns),
            Some("Kontoauszuege")
        );
        assert_eq!(
            match_pattern("Kreditkartenabrechnung vom 21. August 2026", &patterns),
            Some("Kreditkartenabrechnungen")
        );
        assert_eq!(match_pattern("Etwas ganz anderes", &patterns), None);
    }

    #[test]
    fn match_pattern_in_text_ignores_anchor() {
        let patterns = vec![CategoryPattern {
            pattern: r"^Kontoauszug".into(),
            label: "Kontoauszuege".into(),
        }];
        // Simuliert extrahierten PDF-Text: Betreff steht nicht am Anfang.
        let pdf_text = "Herrn Max Mustermann Musterstr. 1\n\nKontoauszug 9/2026 vom 07.09.2026";
        assert_eq!(match_pattern(pdf_text, &patterns), None);
        assert_eq!(
            match_pattern_in_text(pdf_text, &patterns),
            Some("Kontoauszuege")
        );
    }

    #[test]
    fn extracts_date_from_either_format() {
        assert_eq!(
            extract_date("Kontoauszug 9/2026 vom 07.09.2026 zu Konto 123"),
            Some("2026-09-07".to_string())
        );
        assert_eq!(
            extract_date("statementDate 2026-08-21 irgendwas"),
            Some("2026-08-21".to_string())
        );
        assert_eq!(extract_date("kein Datum hier"), None);
    }

    #[test]
    fn extracts_depot_number() {
        assert_eq!(
            extract_depot_number("Abrechnung Kauf ... zu Depot 222222222 Ordernr. 1"),
            Some("222222222".to_string())
        );
        assert_eq!(extract_depot_number("kein Depot hier"), None);
    }

    #[test]
    fn extracts_card_last4() {
        assert_eq!(
            extract_card_last4("Kreditkarte_4930XXXXXXXX2523_Abrechnung_20260821.pdf"),
            Some("2523".to_string())
        );
        assert_eq!(extract_card_last4("Kontoauszug_9_2026.pdf"), None);
    }

    /// Dieselben 28 echten Titel, mit denen die JS-Fassung in
    /// DKB_Dateiabruf/config.mjs gegengeprueft wurde (11.09.2026) --
    /// deckt jede dort beobachtete Dokumentart im echten Postfach ab
    /// (468 Dokumente). Stellt sicher, dass der Rust-Port dasselbe
    /// Verhalten hat wie das Original, insbesondere die Sonderfaelle:
    /// "Visa Kreditkartenvertrag" (Schluesselwort nicht am Anfang) und
    /// "Kaptalmaßnahme" (Schreibvariante der DKB selbst, kein Tippfehler
    /// von uns).
    #[test]
    fn category_matches_all_known_real_titles() {
        use crate::settings::Settings;
        let patterns = Settings::default().category_patterns;

        let cases: &[(&str, &str)] = &[
            ("PRIIP-Verordnung WKN A1JJTC vom 30.11.2025 zu Depot 222222222", "Wertpapierdokumente"),
            ("Abrechnung Kauf WKN A1JJTC vom 07.09.2026 zu Depot 333333333 Ordernr. 1641774200", "Wertpapierdokumente"),
            ("Kontoauszug 9/2026 vom 07.09.2026 zu Konto 9999999999", "Kontoauszuege"),
            ("Kosteninformation zu Wertpapier MUL AMUNDI MSCI AC WORLD UCITS ETF INH.ANTEILE ACC vom 07.09.2026, 10:57 zu Depot 333333333", "Wertpapierdokumente"),
            ("Kreditkartenabrechnung vom 21. August 2026", "Kreditkartenabrechnungen"),
            ("Ertragsabrechnung WKN 865985 vom 14.08.2026 zu Depot 222222222", "Wertpapierdokumente"),
            ("Depotauszug vom 01.07.2026 zu Depot 111111111", "Wertpapierdokumente"),
            ("Kapitalmaßnahme WKN A1T8FV vom 08.07.2026 zu Depot 222222222", "Wertpapierdokumente"),
            ("Kaptalmaßnahme WKN A0CACX vom 13.09.2024 zu Depot 222222222", "Wertpapierdokumente"),
            ("Hauptversammlung WKN 723132 vom 07.05.2026 zu Depot 222222222", "Wertpapierdokumente"),
            ("Legitimation Vollmacht", "Vertragsinformationen"),
            ("Bestätigung der Vollmacht", "Vertragsinformationen"),
            ("Erträgnisaufstellung 2025 Max Mustermann", "Steuerbescheinigungen"),
            ("Jahressteuerbescheinigung 2025 Max Mustermann", "Steuerbescheinigungen"),
            ("Mitteilung über sinkende Sollzinssätze ab 01.10.2025", "Mitteilungen"),
            ("Zustimmung Eröffnung DKB-Broker u18", "Vertragsinformationen"),
            ("Vertragsentwurf DKB-Broker u18", "Vertragsinformationen"),
            ("Vorvertragliche Informationen DKB-Broker u18", "Vertragsinformationen"),
            ("Visa Kreditkartenvertrag", "Vertragsinformationen"),
            ("Vorvertragliche Informationen Visa Kreditkarte", "Vertragsinformationen"),
            ("Änderungsangebot zum 07.10.2025", "Vertragsinformationen"),
            ("Vertrag Tagesgeldkonto", "Vertragsinformationen"),
            ("Vertrag Visa Debit Card", "Vertragsinformationen"),
            ("Vertrag Girokonto U18", "Vertragsinformationen"),
            ("Vereinbarung Girokonto U18", "Vertragsinformationen"),
            ("Informationsbogen für Einleger", "Vertragsinformationen"),
            ("Abrechnung Verkauf WKN A0CACX vom 09.12.2024 zu Depot 222222222 Ordernr. 3507597900", "Wertpapierdokumente"),
            ("Auftragsbestätigung vom 24.10.2024 zu Depot 222222222 - Ordernr. 3692238600", "Wertpapierdokumente"),
        ];

        for (title, expected) in cases {
            assert_eq!(
                match_pattern(title, &patterns),
                Some(*expected),
                "Titel: {title}"
            );
        }
    }

    #[test]
    fn securities_sub_matches_all_known_real_titles() {
        use crate::settings::Settings;
        let patterns = Settings::default().securities_sub_patterns;

        let cases: &[(&str, &str)] = &[
            ("Abrechnung Kauf WKN A1JJTC vom 07.09.2026 zu Depot 333333333 Ordernr. 1641774200", "Kauf"),
            ("Abrechnung Verkauf WKN A0CACX vom 09.12.2024 zu Depot 222222222 Ordernr. 3507597900", "Verkauf"),
            ("Kosteninformation zu Wertpapier MUL AMUNDI MSCI AC WORLD UCITS ETF vom 07.09.2026", "Informationen"),
            ("PRIIP-Verordnung WKN A1T8FV vom 30.11.2025 zu Depot 222222222", "Informationen"),
            ("Ertragsabrechnung WKN 865985 vom 14.08.2026 zu Depot 222222222", "Ertrag"),
            ("Depotauszug vom 01.07.2026 zu Depot 111111111", "Depotauszuege"),
            ("Kapitalmaßnahme WKN A1T8FV vom 08.07.2026 zu Depot 222222222", "Kapitalmassnahmen"),
            ("Kaptalmaßnahme WKN A0CACX vom 13.09.2024 zu Depot 222222222", "Kapitalmassnahmen"),
            ("Hauptversammlung WKN 723132 vom 07.05.2026 zu Depot 222222222", "Hauptversammlung"),
            ("Auftragsbestätigung vom 24.10.2024 zu Depot 222222222 - Ordernr. 3692238600", "Auftragsbestaetigungen"),
        ];

        for (title, expected) in cases {
            assert_eq!(
                match_pattern(title, &patterns),
                Some(*expected),
                "Titel: {title}"
            );
        }
    }

    #[test]
    fn owner_lookup() {
        let owners = vec![DepotOwner {
            depot_number: "12345".into(),
            name: "Testperson".into(),
        }];
        assert_eq!(owner_for(Some("12345"), &owners), Some("Testperson"));
        assert_eq!(owner_for(Some("99999"), &owners), None);
        assert_eq!(owner_for(None, &owners), None);
    }

    #[test]
    fn safe_segment_transliterates_umlauts() {
        assert_eq!(safe_segment("Müller", 80), "Mueller");
        assert_eq!(safe_segment("Straße", 80), "Strasse");
    }

    #[test]
    fn safe_segment_replaces_forbidden_chars() {
        assert_eq!(
            safe_segment("Kontoauszug 9/2026 vom 07.09.2026", 80),
            "Kontoauszug_9_2026_vom_07.09.2026"
        );
    }

    #[test]
    fn safe_segment_keeps_extension_after_truncation() {
        let long_name = "a".repeat(100) + ".pdf";
        let result = safe_segment(&long_name, 10);
        assert_eq!(result, "aaaaaaaaaa.pdf");
    }

    /// Regressionstest fuer einen echten Fund beim Testen des
    /// Sortieren-Nebentools (11.09.2026): Ein Dokument, dessen API-Titel
    /// "Ertragsabrechnung ..." lautet, enthaelt dieses Wort im gedruckten
    /// PDF ueberhaupt nicht -- dort steht statt dessen "Vorabpauschale
    /// Investmentfonds". Ohne dieses Schluesselwort landet das Dokument
    /// beim PDF-Text-Fallback kategorielos direkt im Jahresordner.
    #[test]
    fn vorabpauschale_matches_default_patterns() {
        let settings = crate::settings::Settings::default();
        let text = "Vorabpauschale Investmentfonds\n\nNominale Wertpapierbezeichnung";
        assert_eq!(
            match_pattern_in_text(text, &settings.category_patterns),
            Some("Wertpapierdokumente")
        );
        assert_eq!(
            match_pattern_in_text(text, &settings.securities_sub_patterns),
            Some("Ertrag")
        );
    }
}
