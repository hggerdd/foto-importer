# Architecture Decision Records

Diese Sammlung erklärt, **warum die Architektur so aufgebaut ist** und welche
Randbedingungen bei ihrer Weiterentwicklung erhalten bleiben müssen.
Bedienungsanleitungen und Spezifikationen bleiben unter `docs/`.

## Rückblickende Analyse

Die ADRs 0002–0020 wurden am **27.10.2026** aus dem implementierten Stand
`b82c45e`, den vorhandenen Tests, der Git-Historie und der Projektdokumentation
rekonstruiert. Sie beschreiben geltende Bestandsentscheidungen. Das ursprüngliche
Entscheidungsdatum und damalige Beteiligte werden nicht behauptet. Genannte
Alternativen ordnen die heutige Entscheidung ein; sie belegen keine historische
Evaluation dieser Alternativen. ADR 0001 führt den Dokumentationsprozess neu ein.

Das Programm ist eine lokale, modular aufgebaute Desktop-Anwendung. Ein
schreibgeschützter Foto-Bestand, stabile Foto-/Personen-Identitäten und eine
bedienbare Oberfläche während langer Arbeiten prägen die Architektur. Die
Gesichtsfunktion erweitert diese Basis: vortrainierte lokale Modelle erzeugen
Vektoren, bestätigte Beispiele liefern Namensvorschläge, und Nutzerentscheidungen
haben Vorrang. Die Benennung erfolgt gruppenweise statt über einzelne Kacheln.

Nicht als bereits umgesetzt dokumentiert sind unter anderem Cloud-Synchronisation,
ein HTTP-Backend, GPU-Inferenz, Netzwerktraining, ein externer Nacht-Scheduler,
atomare JSON-Einstellungen oder eine Garantie von 98 % Erkennungspräzision.
Auch ein plötzlich abgebrochener Kopierprozess hat keine garantierte atomare
Veröffentlichung seiner Zieldatei. Die jeweiligen ADRs benennen diese Grenzen.

## Verzeichnis

| ADR | Entscheidung | Status |
| --- | --- | --- |
| [0001](0001-adr-prozess.md) | Architekturentscheidungen dokumentieren und beachten | Akzeptiert |
| [0002](0002-modulare-desktop-anwendung.md) | Python/Tkinter mit getrennten UI-, Core- und Config-Modulen | Akzeptiert |
| [0003](0003-ui-thread-und-worker.md) | Worker-Ergebnisse ausschließlich im Tk-Thread anzeigen | Akzeptiert |
| [0004](0004-vorrang-interaktiver-arbeit.md) | Interaktive Arbeit vor begrenzten Hintergrundaufträgen | Akzeptiert |
| [0005](0005-nichtdestruktiver-import.md) | Originale kopieren, Kollisionen und Teilfehler abfangen | Akzeptiert |
| [0006](0006-datum-und-metadaten.md) | Datumsquelle wählbar, Sammlungsmetadaten über ExifTool | Akzeptiert |
| [0007](0007-dng-konvertierung-als-prozess.md) | Adobe DNG Converter als optionaler externer Prozess | Akzeptiert |
| [0008](0008-lokale-sqlite-sammlung.md) | SQLite im Sammlungsordner mit Migrationen und einem Schreiber | Akzeptiert |
| [0009](0009-stabile-foto-identitaeten.md) | Foto-Identität von Pfad und Dateivariante trennen | Akzeptiert |
| [0010](0010-gestufte-bestandspruefung.md) | Schnellscan, Metadaten und BLAKE3-Prüfung trennen | Akzeptiert |
| [0011](0011-begrenzte-bildvorschauen.md) | Importgalerie nach Sichtbereich laden und cachen | Akzeptiert |
| [0012](0012-lesende-metadatenansicht.md) | Metadaten auf Foto-Ebene lesend und seitenweise abfragen | Akzeptiert |
| [0013](0013-konfigurationsbereiche.md) | App-, Sammlungs- und Modellkonfiguration trennen | Akzeptiert |
| [0014](0014-lokale-austauschbare-gesichtsmodelle.md) | Lokale CPU-Modelle über geprüfte Manifeste laden | Akzeptiert |
| [0015](0015-fortsetzbare-gesichtspipeline.md) | Gesichter quellbezogen und modellabhängig verarbeiten | Akzeptiert |
| [0016](0016-namensvorschlaege-aus-referenzen.md) | Bestätigte Referenzen statt selbstverstärkender Auto-Labels | Akzeptiert |
| [0017](0017-kuratierter-referenzdatensatz.md) | Referenzen balancieren; Exporte aus der DB reproduzieren | Abgelöst durch 0024 |
| [0018](0018-gruppenweise-benennung.md) | Gruppen inline benennen, prüfen und rückgängig machen | Abgelöst durch 0023/0025 |
| [0019](0019-personen-und-beziehungsgraph.md) | Personen-IDs und kanonische Grundbeziehungen speichern | Akzeptiert |
| [0020](0020-reproduzierbare-validierung.md) | Offline-Tests und echte Modell-/Leistungsmessungen trennen | Akzeptiert |
| [0021](0021-uninteressierte-gesichtsgruppen.md) | Gruppen ignorieren und Gesichtsvorkommen erhalten | Akzeptiert |
| [0022](0022-indizierte-sammlungsprojektionen.md) | Indizierte und begrenzte Sammlungsabfragen | Akzeptiert |
| [0023](0023-gesichtsgruppen-mit-begrenztem-aufwand.md) | Gesichtsgruppen mit begrenztem Aufwand | Abgelöst durch 0026 |
| [0024](0024-fortsetzbarer-gesichtsabgleich.md) | Fortsetzbarer Abgleich nach Nutzerentscheidungen | Akzeptiert |
| [0025](0025-sichtbare-gesichtsvorschauen.md) | Gesichtsvorschauen nur im Sichtbereich laden | Akzeptiert |
| [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md) | Vollständig sichtbare Gruppen: höchstens 14 Bilder, größere zuerst | Akzeptiert |
| [0027](0027-sichtbarer-erkennungsstand.md) | Erkennungswissen, Anwendung und Fortschritt sichtbar bedienen | Akzeptiert |
| [0028](0028-kombinierte-bildsuche.md) | Personen und Metadaten in einer begrenzten Bildsuche kombinieren | Akzeptiert |
| [0029](0029-schneller-gesichtsstart-und-schema-reparatur.md) | Lesender Gesichtsstart und Reparatur unvollständiger Skalierungsstände | Akzeptiert |
| [0030](0030-gemeinsame-sammlungs-schreibwarteschlange.md) | Eigene Sammlungs-Schreiber ordnen, externe Sperren erhalten | Akzeptiert |
| [0031](0031-inkrementelle-gesichtsgruppen-ansicht.md) | Bestehende Gruppen, Eingaben und Bilder bei Bestätigung weiterverwenden | Akzeptiert |
| [0032](0032-metadaten-ordnerbaum-und-gesichtsrahmen.md) | Metadaten im begrenzten Ordnerbaum mit quellbezogenen Gesichtsrahmen | Akzeptiert |
| [0033](0033-gesichtskorrekturen-am-bild.md) | Einzelne Fehldetektionen löschen und Gesichter direkt am Bild benennen | Akzeptiert |
| [0034](0034-modellvorschlaege-im-namensdialog.md) | Sechs Modellvorschläge und schnelle Namenssuche im Einzelgesichts-Dialog | Akzeptiert |

## Arbeitsweise ab jetzt

1. Vor einer Änderung diesen Index und die betroffenen akzeptierten ADRs lesen.
   Im Arbeitsplan bzw. in der Änderungsbeschreibung die relevanten Nummern nennen.
2. Für eine neue oder geänderte wesentliche Entscheidung einen eigenen ADR
   anhand der [Vorlage](TEMPLATE.md) anlegen: nächste freie vierstellige Nummer,
   kurzer deutscher Dateiname, genau eine zusammenhängende Entscheidung.
   Dazu zählen Technologie-/Abhängigkeitswahl, Datenhaltung, Schnittstellen,
   Nebenläufigkeit, Datenschutz, Betriebsmodell und prägende Bedienabläufe.
   Reine Formatierungen und Implementierungsdetails innerhalb bestehender ADRs
   benötigen keinen zusätzlichen ADR.
3. Entscheidung, Alternativen, Folgen, Grenzen, betroffene bestehende ADRs und
   überprüfbare Code-/Testbelege dokumentieren. Vorgeschlagene Arbeit nicht als
   bereits umgesetzt ausgeben. Ungeklärte Entscheidungen beginnen als
   **Vorgeschlagen**, geltende Entscheidungen als **Akzeptiert**.
4. Widerspricht eine Änderung einem akzeptierten ADR, die Ablösung explizit
   begründen. Einen neuen ADR anlegen, beim alten den Status **Abgelöst durch
   ADR-NNNN** mit Link setzen und im neuen auf den Vorgänger verweisen. Den alten
   Entscheidungstext und seine Begründung als Historie erhalten. Verworfene
   Vorschläge bleiben mit Status **Verworfen** auffindbar.
5. ADR, Implementierung, relevante Tests, Bedienungsdokumentation und diesen Index
   in derselben Änderung konsistent halten. Im Commit-/PR-Kontext betroffene und
   neue bzw. abgelöste ADRs sowie die Validierung nennen. Eine Bestandsentscheidung
   ist kein Beweis, dass jede Codezeile sie bereits fehlerfrei umsetzt.

Nummern werden nicht wiederverwendet. Neue technische Erkenntnisse können durch
ergänzende Notizen mit Datum festgehalten werden; eine umgekehrte Entscheidung
erfordert einen neuen ADR. Die verbindlichen Agentenregeln stehen in
[AGENTS.md](../AGENTS.md) und die [CLAUDE.md der Python-Referenz](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/CLAUDE.md).

## Rust-Port ab 2026-11-04

Die ADRs 0001–0034 dokumentieren die Python-Referenz; deren Codebelege verlinken den unveränderlichen Quellstand. Im Rust-Port gelten ergänzend:

| ADR | Entscheidung | Status |
| --- | --- | --- |
| [0035](0035-rust-desktop.md) | Rust / egui; löst Technologie aus 0002/0003 ab | Akzeptiert |
| [0036](0036-vollstaendige-kopien.md) | Vollständige Kopien; ersetzt Veröffentlichung aus 0005 | Akzeptiert |
| [0037](0037-sammlungskompatibilitaet.md) | Schema-6-Kompatibilitätsgrenze | Abgelöst durch 0042 |
| [0038](0038-gemeinsamer-arbeitsbereich.md) | Gemeinsamer Arbeitsbereich nach Konzeptkritik | Akzeptiert |
| [0039](0039-rust-onnx-adapter.md) | Lokale Rust-ORT-Adapter mit getrennten Schlüsseln | Akzeptiert (experimentell) |
| [0040](0040-reproduzierbarer-rust-validierungssplit.md) | Reproduzierbarer Split im eigenen Modellraum | Akzeptiert |
| [0041](0041-native-adobe-versionspruefung.md) | Native tägliche Adobe-Versionsprüfung | Akzeptiert |
| [0042](0042-native-bestandsmigrationen.md) | Native Bestandsmigrationen; löst 0037 ab | Akzeptiert |
| [0043](0043-portables-release-paket.md) | Portables Windows-Release-Paket | Akzeptiert |
| [0044](0044-onnx-runtime-download.md) | ONNX Runtime auf Klick im Benutzerprofil installieren | Akzeptiert |
| [0045](0045-fotofokus-in-der-sammlung.md) | Kompakte Sammlungsleiste mit Galerie und großem Inspektor | Akzeptiert |
| [0046](0046-gesichtsmodelle-installieren.md) | Standard-Gesichtsmodelle auf Klick im Benutzerprofil installieren | Akzeptiert |
| [0047](0047-gesichter-als-suchmetadaten.md) | Gesichter und Personen als Fotometadaten anzeigen und suchen | Akzeptiert |
| [0048](0048-erkennungsablauf-im-personenbereich.md) | Analyse, Referenzauswahl und Abgleich sichtbar trennen | Akzeptiert |
| [0049](0049-gewichteter-beziehungsgraph.md) | Beziehungsgraph nach Anzahl gemeinsamer Fotos gewichten | Akzeptiert; Darstellung teilweise abgelöst durch 0058 |
| [0050](0050-empfindlichere-gesichtspraesenz.md) | Kleine und schwache Gesichter für die Präsenzsuche erfassen | Akzeptiert |
| [0051](0051-trainierter-namensklassifikator.md) | Trainierter Softmax-Klassifikator für die Top-3-Namensvorschläge | Akzeptiert |
| [0052](0052-stabile-bildtexturen.md) | Bildtexturen stabil im Cache halten; ergänzt 0011/0025 | Akzeptiert |
| [0053](0053-kleine-gruppen-zusammenfuehren.md) | Kleine Gesichtsgruppen an ähnliche Gruppen anschließen | Akzeptiert |
| [0054](0054-namensmerkmale-aus-hoher-aufloesung.md) | Namensmerkmale kleiner, klar erkannter Gesichter aus höherer Auflösung | Akzeptiert |
| [0055](0055-foto-vorschau-unter-der-maus.md) | Ganzes Foto als Vorschau unter der Maus im Personenbereich | Akzeptiert |
| [0056](0056-breite-gruppen-nutzen-die-fensterbreite.md) | Gruppen bis 28 Gesichter, Kacheln nach Fensterbreite | Akzeptiert |
| [0057](0057-ordnerbaum-in-der-sammlung.md) | Lazy geladener Ordnerbaum als Seitenfeld der Sammlung | Akzeptiert |
| [0058](0058-personengraph-als-netzwerk.md) | Personengraph als Netzwerk im Reiter „Graph“; ergänzt 0049 | Akzeptiert |
| [0059](0059-arbeitsportionen-neueste-zuerst.md) | Analyse und Metadaten in Portionen, neueste zuerst | Akzeptiert |
| [0060](0060-beziehungstypen-und-ableitung.md) | Beziehungstypen, Richtungsunabhängigkeit und Ableitung; ergänzt 0019 | Akzeptiert; Ableitung und Geschwister/Cousin teilweise abgelöst durch 0061 |
| [0061](0061-stammbaum-als-normalisiertes-modell.md) | Stammbaum: nur Basisfakten gespeichert, Verwandtschaft berechnet, Rückfragen, Stammbaumansicht | Akzeptiert |
| [0062](0062-zentraler-personenbestand.md) | Zentraler Personenbestand im Benutzerprofil, Abgleich vor dem Anlegen | Akzeptiert |
| [0063](0063-personentabelle-und-tippfilter.md) | Personentabelle mit Detailspalte und Tippfilter-Auswahlfeldern; ergänzt 0038/0045, ersetzt die Bedienung der Personenliste und Beziehungseingabe aus 0060/0061 | Akzeptiert |
| [0064](0064-gestufte-indizierung-mit-fortschritt.md) | Gestufte Indizierung in Stapeln, ehrlicher Fortschritt mit Phasen/Rate/Restzeit; ergänzt 0010 und 0059 | Akzeptiert |
| [0065](0065-zentrale-beziehungen.md) | Beziehungen zentral speichern und abgleichen (Schema 2, Grabsteine, Rückfragen); setzt Phase 2 aus 0062 um | Akzeptiert |
