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
| [0017](0017-kuratierter-referenzdatensatz.md) | Referenzen balancieren; Exporte aus der DB reproduzieren | Akzeptiert |
| [0018](0018-gruppenweise-benennung.md) | Gruppen inline benennen, prüfen und rückgängig machen | Akzeptiert |
| [0019](0019-personen-und-beziehungsgraph.md) | Personen-IDs und kanonische Grundbeziehungen speichern | Akzeptiert |
| [0020](0020-reproduzierbare-validierung.md) | Offline-Tests und echte Modell-/Leistungsmessungen trennen | Akzeptiert |

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
[AGENTS.md](../AGENTS.md) und [CLAUDE.md](../CLAUDE.md).
