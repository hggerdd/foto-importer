# ADR-0053: Kleine Gesichtsgruppen zusammenführen

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md)

## Kontext

Unbekannte Gesichter werden inkrementell gruppiert: Ein Gesicht tritt der ersten
Gruppe bei, in der alle Mitglieder Kosinus 0,6 erreichen (Complete-Link,
andere Gesichter desselben Fotos ausgeschlossen). Dieselbe Person zerfällt
dadurch bei Pose, Licht oder Alter in viele Gruppen mit ein oder zwei Bildern.
Das macht das Benennen mühsam.

## Entscheidung

Nach dem Gruppieren (`grouping::rebuild_groups`, nach Analyse und „Neu
abgleichen“) wird jede Gruppe mit höchstens zwei Gesichtern der ähnlichsten
anderen Gruppe angeschlossen. Kandidaten kommen aus den acht nächsten
Gruppenzentren. Eine Verschmelzung erfordert: mittlere Paarähnlichkeit
mindestens 0,5 (einstellbar über `rust.thresholds.<Modell>.group_merge`),
kein Paar unter 0,3, keine zwei Gesichter desselben Fotos und höchstens
14 Mitglieder danach. Gewählt wird die höchste mittlere Ähnlichkeit.
Nur abgeleitete Gruppen ändern sich; Namen, Gesichter und Embeddings nicht.
Die größere Gruppe steht dann nach 0026 weiter vorn, abgewählte Ausreißer
bleiben abwählbar.

## Alternativen

Den Complete-Link-Wert global zu senken würde auch große Gruppen verunreinigen.
Ein vollständiges Neu-Clustering verwirft stabile Gruppen-IDs. Automatisches
Benennen kleiner Gruppen verstößt gegen 0016.

## Folgen und Grenzen

Zusammengeführte Gruppen sind nicht mehr streng Complete-Link und können
einzelne falsche Gesichter enthalten, die man vor dem Bestätigen abwählt. Die
Schwellen 0,5 und 0,3 sind **nicht an echten Sammlungen gemessen**. Der Nachlauf
rechnet über alle Gruppen und läuft im Worker. Bestehende Sammlungen
profitieren erst nach dem nächsten „Neu abgleichen“ oder einer Analyse.

## Bei Weiterentwicklung beachten

Schwellen an einer echten Sammlung prüfen (Anteil falscher Mitglieder in
verschmolzenen Gruppen). Gruppen-IDs und Obergrenze 14 erhalten.

## Belege und Validierung

[Nachlauf](../src/core/grouping.rs), [Test](../tests/faces.rs).
