//! Verständliche Fehlermeldungen: ein Satz für die Oberfläche, der Originaltext für den Verlauf.

use super::import_view::display_path;

/// Regeln in Prüfreihenfolge: Teilstrings (ohne Groß-/Kleinschreibung) und der Satz dazu.
const RULES: [(&[&str], &str); 4] = [
    (
        &["unable to open database file"],
        "Die Sammlung konnte nicht geöffnet werden. Ist der Ordner erreichbar?",
    ),
    (
        &["database is locked", "sperr"],
        "Die Sammlung wird gerade von einem anderen Vorgang benutzt. Bitte gleich noch einmal versuchen.",
    ),
    (
        &["exiftool"],
        "ExifTool wurde nicht gefunden. Pfad unter Einstellungen prüfen.",
    ),
    (
        &["no such file", "os error 2", "os error 3"],
        "Eine Datei oder ein Ordner wurde nicht gefunden.",
    ),
];

/// Liefert den Satz für den Nutzer und den Originaltext für „Details“. Unbekannte Fehler
/// bleiben wörtlich erhalten. Pfade erscheinen in beiden ohne das Präfix `\\?\`.
pub(super) fn friendly(raw: &str) -> (String, String) {
    let detail = plain_paths(raw);
    let lower = detail.to_lowercase();
    let sentence = RULES
        .iter()
        .find(|(needles, _)| needles.iter().any(|needle| contains(&lower, needle)))
        .map_or_else(|| detail.clone(), |(_, sentence)| (*sentence).to_owned());
    (sentence, detail)
}

/// „os error 2“ darf nicht auf „os error 21“ passen.
fn contains(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(start, _)| {
        !needle.ends_with(|c: char| c.is_ascii_digit())
            || !text[start + needle.len()..].starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Wendet `display_path` auf jeden Pfad mit erweitertem Präfix im Text an.
fn plain_paths(raw: &str) -> String {
    let mut text = raw.to_owned();
    let mut from = 0;
    while let Some(found) = text[from..].find(r"\\?\") {
        from += found;
        let tail = display_path(&text[from..]).into_owned();
        text.truncate(from);
        text.push_str(&tail);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::friendly;

    #[test]
    fn known_errors_get_a_sentence_and_keep_the_original() {
        for (raw, start) in [
            (
                "unable to open database file: D:\\x",
                "Die Sammlung konnte nicht geöffnet",
            ),
            ("Error: Database is LOCKED", "Die Sammlung wird gerade"),
            ("Die Sammlung ist gesperrt", "Die Sammlung wird gerade"),
            ("No such file or directory", "Eine Datei oder ein Ordner"),
            ("Zugriff: (os error 2)", "Eine Datei oder ein Ordner"),
            (
                "Das System kann den Pfad nicht finden. (os error 3)",
                "Eine Datei oder ein Ordner",
            ),
            (
                "ExifTool konnte nicht gestartet werden (os error 2)",
                "ExifTool wurde nicht gefunden",
            ),
        ] {
            let (sentence, detail) = friendly(raw);
            assert!(sentence.starts_with(start), "{raw} → {sentence}");
            assert_eq!(detail, raw);
        }
    }

    #[test]
    fn unknown_errors_stay_verbatim() {
        let raw = "Der Datenträger ist voll (os error 112)";
        assert_eq!(friendly(raw), (raw.to_owned(), raw.to_owned()));
        // „os error 21“ ist kein „os error 2“.
        assert_eq!(
            friendly("Gerät nicht bereit (os error 21)").0,
            "Gerät nicht bereit (os error 21)"
        );
    }

    #[test]
    fn paths_lose_the_extended_prefix() {
        let (sentence, detail) =
            friendly(r"Kopie nach \\?\D:\Fotos\a.jpg und \\?\UNC\nas\bilder abgelehnt");
        assert_eq!(
            detail,
            r"Kopie nach D:\Fotos\a.jpg und \\nas\bilder abgelehnt"
        );
        assert_eq!(sentence, detail);
    }
}
