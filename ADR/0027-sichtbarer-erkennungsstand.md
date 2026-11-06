# ADR-0027: Erkennungswissen und Abgleich sichtbar bedienen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-29
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0016](0016-namensvorschlaege-aus-referenzen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0025](0025-sichtbare-gesichtsvorschauen.md), [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md)

## Kontext

Benennen und Prüfen erklären bisher nicht, ob persönliches Erkennungswissen
vorhanden ist oder ob Vorschläge bereits mit den neuesten Bestätigungen berechnet
wurden. Der versteckte Datensatz-Neuaufbau vermischt Lernen und Anwendung.
Maschinelle Namen im Fortschrittszähler wirken wie Nutzerbestätigungen.

## Entscheidung

Die Gesichtsansicht zeigt dauerhaft drei Schritte: Fotos analysieren, aus Namen
lernen, Namen vorschlagen. Installierte Basismodelle, bestätigte Gesichter,
geeignete Referenzen und Personen mit Referenzen bleiben getrennte Angaben.
Lernen bedeutet weiterhin Referenzauswahl nach ADR 0016/0024, kein Netztraining.
Automatische Namen zählen ausschließlich als offene Vorschläge, nicht als
Bestätigungen. Die Reiter zählen tatsächlich benennbare bzw. prüfbare Gesichter.
Nicht interessierende Vorkommen bleiben gespeichert und werden separat gezählt.

„Wissen anwenden“ reiht Referenz-Neuaufbau und vollständigen Abgleich
in die bestehende dauerhafte Queue ein. Es werden keine Originalbilder erneut
analysiert. Änderungen der Erkennungseinstellungen invalidieren ebenfalls den
abgeleiteten Stand. Bestätigungen und ignorierte Gesichter bleiben erhalten.
Während eines offenen Abgleichs ist eine zweite Anwendung gesperrt. Pausieren
bewahrt den Cursor; Navigation und Benennung heben die Pause nicht auf.
Fortsetzen setzt vorhandene Arbeit fort. Die Pause selbst gilt für die Sitzung;
nach App-Neustart wird unerledigte Arbeit wieder aufgenommen.

Der letzte vollständige Abschluss wird modellbezogen als UTC-Zeitstempel unter
`recognition_applied` in `face_settings` gespeichert, in derselben Transaktion
wie der Abschluss des Cursors. Offene Arbeit hat bei der Anzeige Vorrang vor
einem früheren Abschluss. Bestandsdaten ohne Abschlussbeleg werden ausdrücklich
als nicht protokolliert dargestellt. Abbruch setzt keinen Abschlusszeitstempel.
Es ist keine Schema-Migration erforderlich.

Der Worker liefert höchstens ungefähr einmal pro Sekunde aggregierten Status;
dieser ersetzt keine Gruppen und zerstört keine Eingabeentwürfe. Veraltete
Sammlungsantworten werden verworfen. Bildladen bleibt sichtbereichsgebunden.
Bei kleineren Fenstern berücksichtigt die Kachelgröße auch die Höhe des
Sichtbereichs, damit eine vollständige 14er-Gruppe Platz behält.

Die Rust-Umsetzung ist in [ADR-0048](0048-erkennungsablauf-im-personenbereich.md)
beschrieben. Dort wird der Abschluss unter einem Embedder-spezifischen
Schlüssel gespeichert.

## Alternativen

Ein separater Trainingsdialog verstärkt die Vorstellung eines manuell trainierten
Netzes und versteckt den laufenden Stand. Ein reiner Text „bereit“ unterscheidet
keine gespeicherten Namen, geeigneten Beispiele und ausstehenden Berechnungen.
Ein synchroner Vollabgleich würde interaktive Entscheidungen blockieren.

## Folgen und Grenzen

Die Oberfläche beansprucht zusätzlichen Platz; kleinere Fenster zeigen kleinere
Vorschauen. Sehr geringe Fensterhöhen benötigen weiterhin globales Scrollen.
Aggregationen lesen Metadaten und zählen Datensätze, laden aber keine globale
Bild- oder Vektorliste. Der Abschluss belegt einen vollständigen Berechnungslauf,
keine Erkennungsqualität oder garantierte Identität. Ungeeignete bestätigte Bilder
können null Referenzen ergeben; die Oberfläche erklärt diesen Zustand.
Der Zeitstempel ist kein Modellartefakt und keine Qualitätsversion.

## Bei Weiterentwicklung beachten

Keinen erfolgreichen Abschluss aus fehlenden Queue-Einträgen allein ableiten.
Modelle getrennt behandeln, Nutzerlabels erhalten und Teilfortschritt atomar
speichern. Kein Neurendern der Gruppen bei jedem Fortschrittsereignis.

## Belege und Validierung

[Status und Anwendung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_recognition.py),
[Transaktionsabschluss](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_service.py),
[Statusanzeige](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_recognition.py), [Worker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py),
[Core-Regressionen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py),
[reale Tk-Interaktionen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py),
[synthetische UI-Vorschau](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/face-recognition-status.png).

