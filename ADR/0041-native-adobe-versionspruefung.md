# ADR-0041: Native Adobe-Versionsprüfung

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0007](0007-dng-konvertierung-als-prozess.md), [0035](0035-rust-desktop.md)

## Kontext

Die Python-Version bietet eine tägliche Prüfung des optionalen DNG Converters.
Rust soll dieselbe Funktion ohne Python oder gestarteten Installer bereitstellen.

## Entscheidung

Reqwest 0.12 mit Rustls führt ausschließlich einen begrenzten HEAD-Request an
den bekannten Adobe-Downloadlink aus: fünf Sekunden Timeout, höchstens acht
Weiterleitungen. Nur HTTPS-Ziele auf download.adobe.com mit dem erwarteten
Windows-Installernamen liefern eine Versionsnummer. Lokale PE-Versionsressourcen
werden über Windows gelesen, ohne den Converter zu starten.
Der lokale Kalendertag wird vor dem Onlineversuch gespeichert, einschließlich
fehlgeschlagener Prüfungen. Es gibt keinen automatischen Download oder Installer.

## Alternativen

Ein Shellaufruf von curl fügt eine externe Laufzeitvoraussetzung hinzu. WinHTTP
direkt würde mehr manuelle FFI- und Lebenszykluslogik erfordern. Eine entfernte
Versionszahl fest einzubauen wäre rasch falsch.

## Folgen und Grenzen

Dies ist die einzige automatische Netzwerkanfrage der App; Fotos, Metadaten und
Embeddings bleiben lokal. Adobe kann sein Linkformat ändern; dann bleibt die
Version unbekannt. Das TLS-Verhalten hängt vom Root-Zertifikatbestand ab.
Ein fehlgeschlagener Schreibzugriff auf die Arbeitsplatzkonfiguration verhindert
die dauerhafte Tagesreservierung und muss sichtbar bleiben.

## Bei Weiterentwicklung beachten

Fehlercaches, Versionsvergleich und Hostprüfung erhalten. Keine neue Prüfung
bei jedem Navigationswechsel. Installation braucht eine eigene Entscheidung.

## Belege und Validierung

[Service](../src/core/dng_updates.rs), [Tests](../tests/dng_updates.rs),
[Reqwest](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.ClientBuilder.html).
