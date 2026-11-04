# ADR-0028: Kombinierte Bildsuche als lesender Baukasten

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-29
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0004](0004-vorrang-interaktiver-arbeit.md), [0008](0008-lokale-sqlite-sammlung.md), [0009](0009-stabile-foto-identitaeten.md), [0012](0012-lesende-metadatenansicht.md), [0021](0021-uninteressierte-gesichtsgruppen.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0025](0025-sichtbare-gesichtsvorschauen.md)

## Kontext

Personen und EXIF-Daten liegen bereits in derselben Sammlung. Es fehlt eine
Foto-Suche mit Kombinationen wie X und Y ohne Z sowie X und Y ohne weitere
Gesichter. Eine vollständige Suchfeldwand erschwert den Einstieg; eine globale
Bildliste würde die Speicherschranken großer Sammlungen umgehen.

## Entscheidung

Ein eigener Tab Bildsuche führt über Suchideen und einen thematisch gegliederten
Baukasten. Der Entwurf wird explizit angewendet; die Suchspur und entfernbare
Kriterien machen Kombinationen sichtbar. Änderungen während laufender Abfragen
werden nicht stillschweigend als bereits angewendet dargestellt. Personengruppen
für alle, mindestens eine und keine werden mit UND verbunden. Metadatenkriterien
bilden eine zusätzliche UND- oder ODER-Gruppe; Text und Zeitraum bleiben UND.
Erkennungsvorschläge sind opt-in; Standard sind bestätigte Identitäten.

Personenpräsenz bezieht sich auf aktuelle Vorkommen (`retained=0`), unabhängig
vom Interesse. Ausschlüsse und Nur-diese-Personen benötigen einen erfolgreichen
Detektionsauftrag des aktiven Detektors. Unanalysierte Fotos gelten nicht als
personenfrei. Eine weitere aktuelle Detektion, auch ignoriert oder unbekannt,
verhindert den strikten Treffer. Bei ausschließlich erforderlichen Personen
wird zusätzlich genau ein Vorkommen pro ausgewählter Person verlangt.
Aufbewahrte historische Detektionen sind kein aktueller Präsenzbeleg.

Der Kern übersetzt unveränderliche Filter in gebundene SQL-Parameter; Feldnamen,
Vergleiche und Sortierung kommen ausschließlich aus festen Positivlisten.
Ein lesender SQLite-Snapshot fragt `metadata_photos` und indizierte EXISTS-
Unterabfragen auf Foto-ID ab. Korrelierte Gesichtsabfragen erzwingen den
vorhandenen `ix_faces_photo`: Der Lasttest mit häufigen Personen zeigte sonst
quadratische Arbeit durch die Wahl von `ix_faces_person`. Der Abfrageplan ist
als Regression abgesichert. Vorhandene Indizes aus 0022 reichen aus; keine
Migration und keine zusätzlichen Bild-/Vektorkopien. Alte Schemas vor v4 zeigen
einen Upgrade-Hinweis statt einer Migration durch die Suche. Ergebnisse zählen
Foto-IDs, nicht Varianten. Formatkriterien betrachten alle vorhandenen Dateien;
Sterne/Farbe übernehmen die XMP-Priorität des vorhandenen Detailbrowsers.

Die Seite umfasst höchstens 48 Metadatenzeilen. SQLite nutzt 16 MiB Seitencache
und dateibasierten temporären Speicher. Nur sichtbare Kacheln erhalten dekodierte
Bilder und Tk-Bildobjekte; vorhandener begrenzter Thumbnailcache und PreviewWorker
werden wiederverwendet. RAW-Vorschauen werden aus JPEG-Paaren oder eingebetteten
Bildern gelesen. Queries und Details laufen in getrennten einzelnen Workern;
neue Queries brechen alte SQLite-Abfragen über einen Fortschrittshandler ab.
Interaktive Suche, Details und Vorschauen reservieren den gemeinsamen Scheduler;
auch abgebrochene wartende Futures geben ihre Reservierung frei.
Sammlungs-/Ansichtsrevisionen verwerfen verspätete Ergebnisse. Suchdetailfenster
und Worker werden beim Schließen abgekoppelt.

## Alternativen

Eine Freitext-Pseudosprache wäre ohne Parser und erklärbare Semantik mehrdeutig.
Eine reine Metadatenfilterliste erklärt Gruppen, Ausschlüsse und unbekannte
Gesichter nicht. Vollständige Ergebnis-/Bildlisten im Python-Speicher oder ein
neuer Suchserver widersprechen dem lokalen, begrenzten Betriebsmodell.

## Folgen und Grenzen

„Kein weiteres erkanntes Gesicht“ kann keine Abwesenheit übersehener Menschen
beweisen. Der gespeicherte Auftrag belegt die letzte Analyse des gewählten
Detektors; nach Originaländerungen ist eine erneute Analyse erforderlich.
Bei ausgeschlossenen Personen gilt die gewählte Identitätssicherheit ebenfalls:
Ein unbestätigtes Z ist standardmäßig kein bestätigtes Z. Die Oberfläche erklärt
die Vorschlagsoption und die Grenzen der Erkennung.

Freie Teilstringsuche, beliebige Vergleiche und tiefe OFFSET-Seiten können
Datenbankarbeit benötigen, laden aber keine globale Bildliste. GPS-Koordinaten
werden ohne Ortsnamen-Geocoding genutzt. Unbekannte Metadaten sind nicht null
im Sinne des Zahlenwerts 0. Die Regeln unterstützen keine frei verschachtelte
Boolesche Sprache; die dokumentierten Gruppen sind die explizite Semantik.

## Bei Weiterentwicklung beachten

Keine Interesse-Flags als Präsenzfilter verwenden; keine historischen Detektionen
mitzählen. Identitätsstatus in Positiv- und Negativbedingungen konsistent halten.
Ergebnisbegrenzung, UI-Thread-Grenze und verworfene alte Antworten testen.
Neue Suchfelder müssen ihre Quell-/Variantenpriorität festlegen. Eine strengere
Analyse-Frischeprüfung oder neue Projektionsindizes benötigt gemessene Begründung.

## Belege und Validierung

[Suchkern](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/photo_search.py), [Thumbnailadapter](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/search_thumbnails.py),
[Baukasten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/photo_search_builder.py), [Galerie/Worker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/photo_search_view.py),
[Core-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_photo_search.py), [Tk-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_photo_search_view.py),
[Bedienung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/bildsuche.md), [synthetische UI-Vorschau](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/photo-search.png),
[Lastmessung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/scripts/benchmark_photo_search.py).

