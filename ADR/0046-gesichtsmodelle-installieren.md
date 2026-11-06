# ADR-0046: Standard-Gesichtsmodelle auf Klick installieren

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-06
- **Entscheidungsdatum:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0004](0004-vorrang-interaktiver-arbeit.md), [0013](0013-konfigurationsbereiche.md), [0014](0014-lokale-austauschbare-gesichtsmodelle.md), [0020](0020-reproduzierbare-validierung.md), [0039](0039-rust-onnx-adapter.md), [0043](0043-portables-release-paket.md) und [0044](0044-onnx-runtime-download.md)

## Kontext

Das portable Paket enthält Modellmanifeste. Der gespeicherte Pfad kann auf
ein älteres Paket zeigen, in dem die Gewichte ebenfalls fehlen. Der Download
der ONNX Runtime allein macht die Gesichtsanalyse deshalb noch nicht nutzbar.

## Entscheidung

Personen und Einstellungen bieten eine explizite Installation von YuNet und
SFace an. Ein abbrechbarer Worker verwendet passende vorhandene Gewichte oder
lädt die fehlenden Dateien von OpenCV Zoo per HTTPS. Bezugsquellen, Größe,
Lizenz und SHA-256 kommen aus den mit der App kompilierten Standardmanifesten.
Der Worker installiert Gewichte und Manifeste gemeinsam in einem neuen Ordner
unterhalb der Arbeitsplatzeinstellungen. Die App speichert und übernimmt den
Ordner erst nach erfolgreicher Prüfung beider Gewichte.

## Alternativen

Eine Anleitung zum manuellen Download lässt Pfadsuche und Dateiplatzierung
offen. Eine Installation neben der EXE hängt vom Speicherort des portablen
Pakets und dessen Schreibrechten ab. Ein Download beim Start oder während
einer Analyse widerspricht der expliziten lokalen Einrichtung aus ADR 0014.

## Folgen und Grenzen

Der installierte Modellordner bleibt bei einem Wechsel des Release-Ordners
nutzbar. Bei einem fehlgeschlagenen oder abgebrochenen Download bleibt die
bisherige Auswahl erhalten. Andere Modelle werden weiter manuell eingerichtet.
Ein Modellwechsel in der Sammlung bleibt eine gesonderte Entscheidung.
Die Dateien werden lokal geladen. Die Installation sendet keine Fotos und
keine biometrischen Daten.

## Bei Weiterentwicklung beachten

Beim Aktualisieren der Standardmodelle Bezugsquelle, Lizenz, Größe und Hash
gemeinsam prüfen. Modellschlüssel und lokale Inferenz gemäß ADR 0039 erhalten.
Offline-Regressionen und echte Downloads getrennt testen.

## Belege und Validierung

[Installationsdienst und Offline-Regression](../src/core/face_models.rs),
[Personen-Tab](../src/ui/app/people_view.rs),
[Einstellungen](../src/ui/app/settings_view.rs),
[Modelltests](../tests/models.rs) und [Modellanleitung](../models/README.md).
