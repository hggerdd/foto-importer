# ADR-0067: Kompletter Ordnerbaum in einem Durchgang

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-13
- **Art:** Änderung von 0057
- **Entscheidungsdatum:** 2026-11-13
- **Bezieht sich auf:** [0042](0042-native-bestandsmigrationen.md), [0057](0057-ordnerbaum-in-der-sammlung.md), [0066](0066-schnelles-speichern-von-gesichtsgruppen.md)

## Kontext

ADR 0057 lud den Baum ebenenweise in Seiten zu 200 Ordnern. Bei großen Sammlungen
dauerte es lange, bis der Baum sichtbar war: Die Wurzelabfrage las über
`ix_files_directory_name` für jede Datei die Tabellenzeile nach (`photo_id`, `id`), und
Ordner jenseits der ersten 200 verlangten den Knopf „weitere …“. Gemessen
(Release, 100.000 Fotos, 20.000 Ordner, 60.000 Verzeichnisse): rund 0,6 bis 0,7 s für
eine Gesamtabfrage über diesen Index.

## Entscheidung

- Der **komplette Baum** wird in einem Workerauftrag gelesen (`folders::directories`:
  Fotos je Verzeichnis, eine Zeile je Foto, bevorzugte Variante, keine Sidecars) und im
  Speicher aufgebaut. Aufklappen, Zuklappen und Blättern greifen nie auf die Datenbank
  zu. Der Knopf „weitere …“ und das Seitenlimit entfallen.
- **Zusatzindex** `ix_files_dir_cover ON files(dir, photo_id, id)`: rein additiv,
  `CREATE INDEX IF NOT EXISTS` beim Öffnen mit Schreibrecht (Muster aus ADR 0066), auch in
  `schema.sql` für neue Sammlungen. Die Abfrage bleibt vollständig im Index. Gemessen
  (Release, synthetisch): 0,07 bis 0,09 s statt 0,6 bis 0,7 s.
- **Ohne den Index** (schreibgeschützt geöffnete ältere Sammlung, nichts wird verändert)
  liefert eine Ersatzabfrage über `metadata_photos.relpath` dasselbe Ergebnis (gemessen
  0,14 s); ein Test vergleicht beide.
- **Aktualisierung ohne Flackern:** Ändert sich der Bestand, bleibt der alte Baum samt
  Aufklappzustand sichtbar, bis der neue geladen ist.
- **Sortierung wie im Windows-Explorer** (`folders::explorer_order`): Sonderzeichen wie `_`
  zuerst, dann Zahlen (nach Wert, `2` vor `10`), dann Buchstaben ohne Beachtung von Groß-/
  Kleinschreibung; Umlaute zählen wie ihr Grundbuchstabe.

## Alternativen

Eine eigene Tabelle mit vorberechneten Ordnerzahlen wäre noch schneller, braucht aber
Trigger und eine Migration (ADR 0042) und lohnt bei 0,1 s nicht. Weiterhin ebenenweise
laden würde Wartezeiten beim Aufklappen behalten.

## Folgen und Grenzen

- Speicher: ein Knoten je Ordner; bei 80.000 Ordnern wenige MB. Nur geöffnete Zeilen
  werden gezeichnet.
- Gemessen wurde nur mit synthetischen Daten, nicht mit einer echten Sammlung und nicht auf
  einem Netzlaufwerk. Der Indexaufbau beim ersten Öffnen mit Schreibrecht kostet bei großen
  Sammlungen einige Sekunden einmalig (nicht gemessen mit echten Daten).
- Die Sortierung folgt nicht exakt `StrCmpLogicalW` (z. B. Sonderzeichenreihenfolge
  untereinander nach Zeichencode).

## Belege

[Kern](../src/core/folders.rs), [Tests](../tests/folders.rs),
[Messbeispiel](../examples/folder_bench.rs), [Seitenfeld](../src/ui/app/folder_tree.rs).
