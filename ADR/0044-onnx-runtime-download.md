# ADR-0044: ONNX Runtime auf Klick installieren

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0004](0004-vorrang-interaktiver-arbeit.md), [0013](0013-konfigurationsbereiche.md), [0020](0020-reproduzierbare-validierung.md), [0039](0039-rust-onnx-adapter.md), [0043](0043-portables-release-paket.md)

## Kontext

Die Gesichtsanalyse braucht die native ONNX Runtime. Nutzer kennen den Pfad
einer DLL häufig nicht. Das portable Paket setzt voraus, dass optionale
Werkzeuge separat bereitgestellt werden.

## Entscheidung

Die App bietet in Personen und Einstellungen einen Download für Windows x64
an. Ein abbrechbarer Worker lädt das festgelegte CPU-Paket 1.22.0 über HTTPS
von Microsofts GitHub-Release. Die Version entspricht der API des eingesetzten
ORT-Bindings. Die App prüft Archivgröße und fest hinterlegte SHA-256-Prüfsumme
vor dem Entpacken. `zip` liest die beiden DLLs und Lizenzdateien anhand fester
Eintragsnamen. Die App übernimmt den DLL-Pfad erst nach vollständigem Entpacken.

Die Installation liegt in einem eigenen Ordner unterhalb der
Arbeitsplatzeinstellungen. Jeder Download erhält einen neuen Ordner, damit
eine bereits geladene DLL weiter nutzbar bleibt. Auswahl und Installation
erfordern keine Administratorrechte. Die manuelle Auswahl bleibt verfügbar.

## Alternativen

Ein Link allein würde Nutzer weiterhin mit Entpacken und Pfadsuche belasten.
Eine gebündelte Runtime würde jeden Download vergrößern. Ein automatisch
gewählter aktueller Release erschwert reproduzierbare Kompatibilitätsprüfungen.

## Folgen und Grenzen

Eine bestehende Installation bleibt beim Fehler und beim Abbruch verwendbar.
Wiederholte Downloads können zusätzlichen Platz belegen. Die Runtime wird nur
auf Klick bezogen. Die separate Gewichtsinstallation regelt [ADR-0046](0046-gesichtsmodelle-installieren.md). Wenn bereits
eine andere Runtime geladen wurde, muss die App nach dem Wechsel neu gestartet werden.

## Bei Weiterentwicklung beachten

Paketversion, Größe und Prüfsumme gemeinsam aktualisieren. Das offizielle Paket
mit dem Rust-Binding prüfen. Download und Entpacken außerhalb des UI-Threads
halten. Vorhandene Installationen erst nach gesonderter Entscheidung bereinigen.

## Belege und Validierung

[Microsoft-Release](https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0),
[Installationsdienst mit Offline-Tests](../src/core/onnx_runtime.rs),
[Netzwerktest](../tests/onnx_runtime.rs), [Einstellungen](../src/ui/app/settings_view.rs).
