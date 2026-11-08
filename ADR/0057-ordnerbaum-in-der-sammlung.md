# ADR-0057: Ordnerbaum in der Sammlung

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-08
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-08
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0028](0028-kombinierte-bildsuche.md), [0032](0032-metadaten-ordnerbaum-und-gesichtsrahmen.md), [0045](0045-fotofokus-in-der-sammlung.md)

## Kontext

Die Sammlungsansicht bot nur ein Menü „Ordner · Alle“ mit den direkten
Unterordnern des gewählten Ordners (200 je Seite). Wer sehen will, welche Ordner
bereits in der Sammlung liegen, musste sich Ebene für Ebene durchklicken und sah
keine Fotozahlen. Die Sammlung soll später über 100.000 Fotos und Tausende
Ordner enthalten; ein vollständig geladener Baum würde Speicher, Abfragezeit
und Zeichenaufwand unnötig erhöhen. Das alte Menü fragte außerdem bei jeder
Suche per `SELECT DISTINCT` über alle Treffer die Unterordner ab.

## Entscheidung

Ein einklappbares Seitenfeld links neben der Galerie zeigt die Ordner als
Baum. Die Wurzel trägt den Namen des Sammlungsordners; ein Klick darauf hebt den
Ordnerfilter auf. Ein Klick auf einen Ordner setzt den bestehenden Ordnerfilter
(`Search::folder`, Präfix auf dem Pfad der bevorzugten Variante) und geht über
die bestehende Seitennavigation auf Seite 1. Der gewählte Ordner ist
hervorgehoben, seine Vorfahren sind aufgeklappt. Das Dreieck klappt auf und zu,
ein Klick auf den Namen wählt. Mit Tab erreicht man die Zeilen, Pfeil rechts und
links klappen auf und zu, Enter wählt. Jede Zeile trägt eine Beschriftung
(„Pfad, N Fotos, aufgeklappt“) für Hilfstechnik.

**Zählung:** Jeder Ordner zeigt die Zahl der Fotos in ihm und allen Unterordnern
(eine Zeile je Foto, wie die Galerie: bevorzugte Variante, keine Sidecars). Die
Zahl stimmt damit mit der Trefferzahl nach der Auswahl überein, solange keine
weiteren Suchkriterien aktiv sind; ein Test prüft die Übereinstimmung. Zahlen
bleiben bewusst unabhängig von Suchtext und Filtern: Der Baum beschreibt den
Bestand, die Galerie die Auswahl. Direkte Fotozahlen werden nicht angezeigt.

**Skalierung:** `core::folders::children` liefert eine Seite (200) der direkten
Unterordner eines Ordners samt Zahl und der Angabe, ob Unterordner existieren.
Ein Ordner wird erst beim Aufklappen geladen; die Wurzel beim Öffnen der
Ansicht. Eine Seite weiter wird mit „weitere …“ angefordert und an die Ebene
angehängt. Das Laden läuft als Worker `folders` (Lesen, abbrechbar über den
SQLite-Fortschrittshandler, Vorrang wie andere Lesearbeit). Pro Worker werden
höchstens 24 Ebenen gelesen, damit auch ein tief gewählter Ordner in einem
Durchgang aufgeklappt wird. Antworten tragen eine Revision; ein Wechsel der
Sammlung oder ihres Bestands (Foto-/Dateizahl) erhöht sie, verspätete Antworten
werden verworfen. Aufgeklappte Ordner bleiben bei Bestandsänderungen erhalten,
geladene Ebenen werden neu gelesen. Fehlgeschlagene Ebenen werden nicht in einer
Schleife wiederholt, sondern bieten „erneut versuchen“. Der Baum zeichnet nur
sichtbare Zeilen (`show_rows`). Die Logik von Ebenen, Aufklappzustand und
sichtbaren Zeilen liegt als `FolderTree` ohne UI-Abhängigkeit im Kern.

**Abfrageplan:** Die Abfrage liest einen Bereich von `files.dir` über den
vorhandenen Index `ix_files_directory_name` (`dir>=? AND dir<?`, die obere
Grenze ersetzt `LIKE` und beachtet keine Platzhalterzeichen) und verbindet über
den Primärschlüssel `metadata_photos.photo_id` auf die bevorzugte Variante.
Gruppiert wird nach dem ersten Segment unterhalb des Elternordners. Es gibt keinen
neuen Index, keine Schemaänderung und damit keine stille Nachrüstung bestehender
Schema-6-Sammlungen. Ein Test prüft den Plan (`EXPLAIN QUERY PLAN`) und misst mit
100.000 Fotos in 20.000 Ordnern (60.000 Unterordner) im Debug-Build rund 0,3 s
für eine Wurzelseite und unter 1 ms für eine Ebene unterhalb eines Ordners.

**Ansicht:** Zustand (offen/zu) und Breite stehen in den Einstellungen
(`folder_tree_open`, `folder_tree_width`, begrenzt auf 170 bis 380 Punkte, Griff
zwischen Baum und Galerie). Der Baum weicht vor der Galerie (mindestens 340) und
dem Bilddetailbereich (mindestens 320): Bei 980 × 680 mit offenen Bilddetails
bleibt er ausgeblendet, die Schaltfläche „Ordnerbaum“ blendet ihn nach dem
Ausblenden der Details ein. Das Menü „Ordner“ und die dafür bei jeder Suche
berechnete Unterordnerliste entfallen; `Search::folder_page` ebenfalls.

## Alternativen

Ein vollständiger Baum aus einer einmaligen Abfrage wäre einfach, lädt bei
Tausenden Ordnern aber alles und blockiert die Anzeige. Eine eigene Tabelle mit
vorberechneten Ordnerzahlen wäre schneller, benötigt aber Trigger und eine
Migration bestehender Sammlungen (ADR 0022 verbietet stille Nachrüstung) und
bleibt eine spätere Option, falls die Wurzelseite bei realen Sammlungen zu
langsam ist. Zahlen unter Berücksichtigung aller Filter würden die vollständige
Suchbedingung je Ebene ausführen; das war für den Bestandsüberblick unnötig.
Ein Dialog wäre bei der Navigation zwischen Ordnern zu umständlich.

## Folgen und Grenzen

Die Wurzelseite und jede Ebene mit vielen Nachkommen lesen alle Dateien im
Verzeichnisbereich; die Kosten wachsen mit der Zahl der Fotos unterhalb, nicht
mit der Gesamtzahl der Ordner. Gemessen wurde nur mit synthetischen Daten im
Debug-Build, nicht mit einer echten Sammlung mit über 100.000 Fotos.
Der gewählte Ordner erscheint nur markiert, wenn er auf der geladenen Seite
seiner Ebene steht; bei mehr als 200 Geschwistern muss „weitere …“ gewählt werden.
Die Anzeige folgt dem Pfad der bevorzugten Variante. Bei RAW-/JPEG-Paaren in
Erweiterungsordnern (z. B. `NEF`, `JPG`) erscheinen diese Ordner im Baum.
Ordnernamen werden ohne Unicode-Normalisierung binär verglichen, die Sortierung
ist nicht von Groß-/Kleinschreibung abhängig. Eine Bedienung mit Pfeil hoch/runter
zwischen Zeilen sowie ein Test mit Screenreader stehen aus. Der Sichttest
erfolgte mit der synthetischen Demo-Sammlung.

## Bei Weiterentwicklung beachten

Die Zählung muss dieselbe Variantenwahl und dieselben Präfixregeln wie
`metadata::search` verwenden; der Test vergleicht beide. Neue Schemaobjekte für
den Baum wären eine Migration gemäß ADR 0042 und ADR 0022. Der Plantest schützt
die Nutzung von `ix_files_directory_name`. Neue Hintergrundarbeit des Baums muss
weiter auf einem Worker mit Revisionsprüfung laufen (ADR 0003).

## Belege und Validierung

[Kern und Baummodell](../src/core/folders.rs), [Kerntests mit 20.000 Ordnern](../tests/folders.rs),
[Seitenfeld](../src/ui/app/folder_tree.rs), [Layout](../src/ui/app/collection_view.rs),
[UI-Test des Workers](../src/ui/app/tests.rs). Geprüft: `cargo fmt --check`,
`cargo clippy -j 1 --all-targets -- -D warnings`, `cargo test -j 1`; Sichtprüfung
per Fensteraufnahme mit der synthetischen Demo-Sammlung (`cargo run --example demo`,
`FOTO_IMPORTER_FOLDER` für den gewählten Ordner).
