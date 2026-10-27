"""Modeless collection index controls and queued background operations."""

from __future__ import annotations

import threading
import tkinter as tk
from datetime import datetime
from pathlib import Path
from tkinter import filedialog, messagebox, simpledialog, ttk

from core.collection_index import CollectionIndex
from ui.dispatcher import UiDispatcher
from ui.metadata_view import MetadataView


class CollectionController:
    """Keep SQLite and filesystem work outside Tk's event loop."""

    def __init__(self, root: tk.Misc, settings, target, on_import_status=None) -> None:
        self.root = root
        self.settings = settings
        self.target = target
        self.on_import_status = on_import_status
        self.dispatcher = UiDispatcher(root)
        self.cancel = threading.Event()
        self.busy = False
        self.window = None
        self.report = None
        self.metadata_view = None
        self.last_result = None
        self.last_root = None
        self._closed = False
        self._night_date = None
        self._inspect_revision = 0
        self._offered = set()
        self._night_timer = root.after(30_000, self._night_tick)
        self._offer_timer = root.after(0, self.offer_index)

    def offer_index(self) -> None:
        """Offer an initial index when selecting an existing collection."""
        root = self.target()
        if (
            root
            and root not in self._offered
            and root.is_dir()
            and not (root / ".myfotomanager").exists()
        ):
            self._offered.add(root)
            self.run("quick_scan", root=root)

    def _night_tick(self) -> None:
        date = datetime.now()
        if (
            date.hour == 3
            and self._night_date != date.date()
            and self.settings.get("collection_auto_metadata", False)
        ):
            self._night_date = date.date()
            root = self.target()
            if root and (root / ".myfotomanager").exists() and not self.busy:
                self.run("enrich_metadata", root=root)
        self._night_timer = self.root.after(30_000, self._night_tick)

    def open_window(self) -> None:
        if self.window and self.window.winfo_exists():
            self.window.lift()
            return
        self.window = tk.Toplevel(self.root)
        self.window.title("Sammlung · Foto-Importer")
        self.window.geometry("1450x950")
        self.window.minsize(1120, 780)
        header = ttk.Frame(self.window, padding=(24, 16))
        header.pack(fill="x")
        ttk.Label(header, text="Sammlung", style="Title.TLabel").pack(anchor="w")
        self.collection_path_var = tk.StringVar(
            value=str(self.target() or "Zielordner auswählen")
        )
        ttk.Label(
            header, textvariable=self.collection_path_var, style="Muted.TLabel"
        ).pack(anchor="w", pady=(4, 0))
        self.notebook = ttk.Notebook(self.window)
        self.notebook.pack(fill="both", expand=True)
        pane = ttk.Frame(self.notebook, padding=16)
        self.notebook.add(pane, text="  Index  ")
        self.metadata_view = MetadataView(
            self.notebook, self.target, lambda: self.run("enrich_metadata")
        )
        self.notebook.add(self.metadata_view, text="  Metadaten  ")
        self.notebook.select(self.metadata_view)
        self.notebook.bind("<<NotebookTabChanged>>", self._tab_changed)
        self.status_var = tk.StringVar(value="Zielordner auswählen und Status laden.")
        ttk.Label(pane, textvariable=self.status_var, wraplength=850).pack(anchor="w")
        buttons = ttk.Frame(pane)
        buttons.pack(fill="x", pady=12)
        for label, command in (
            ("Status", self.refresh),
            ("Änderungen suchen", lambda: self.run("quick_scan")),
            ("Ordner prüfen …", self.scan_folder),
            ("Metadaten ergänzen", lambda: self.run("enrich_metadata")),
            ("Vollständige Prüfung …", lambda: self.run("verify")),
        ):
            ttk.Button(buttons, text=label, command=command).pack(side="left", padx=2)
        options = ttk.Frame(pane)
        options.pack(fill="x")
        ttk.Button(options, text="Abbrechen", command=self.cancel.set).pack(side="left")
        ttk.Button(options, text="Fehlende bereinigen …", command=self.purge).pack(
            side="left", padx=4
        )
        ttk.Button(options, text="Optionen …", command=self.options).pack(
            side="left", padx=4
        )
        ttk.Button(
            options, text="Mehrdeutige Verschiebung …", command=self.resolve
        ).pack(side="left", padx=4)
        auto = tk.BooleanVar(value=self.settings.get("collection_auto_metadata", False))
        ttk.Checkbutton(
            pane,
            text="Automatisch nachts um 03:00 (solange die App läuft)",
            variable=auto,
            command=lambda: self.settings.set("collection_auto_metadata", auto.get()),
        ).pack(anchor="w", pady=8)
        skip = tk.BooleanVar(value=self.settings.get("collection_skip_imported", True))
        ttk.Checkbutton(
            pane,
            text="Bereits importierte und wahrscheinlich importierte Dateien überspringen",
            variable=skip,
            command=lambda: self.settings.set("collection_skip_imported", skip.get()),
        ).pack(anchor="w")
        self.progress_var = tk.StringVar(value="Bereit")
        ttk.Label(pane, textvariable=self.progress_var).pack(anchor="w")
        content = ttk.Frame(pane)
        content.pack(fill="both", expand=True, pady=8)
        self.report = tk.Text(content, wrap="word", state="disabled")
        scrollbar = ttk.Scrollbar(content, command=self.report.yview)
        self.report.configure(yscrollcommand=scrollbar.set)
        scrollbar.pack(side="right", fill="y")
        self.report.pack(fill="both", expand=True)
        self.refresh()

    def _tab_changed(self, event=None) -> None:
        if (
            self.metadata_view
            and not self.metadata_view._closed
            and self.notebook.select() == str(self.metadata_view)
        ):
            self.metadata_view.reload()

    def target_changed(self) -> None:
        """Refresh an open collection browser after choosing another destination."""
        if self.window and self.window.winfo_exists():
            self.collection_path_var.set(str(self.target() or "Zielordner auswählen"))
            self.metadata_view.page = 0
            self.metadata_view.camera.set("Alle Kameras")
            self.metadata_view.year.set("Alle Jahre")
            self.metadata_view.reload()

    def _show(self, text: str) -> None:
        if self.window and self.window.winfo_exists():
            self.report.configure(state="normal")
            self.report.delete("1.0", "end")
            self.report.insert("end", text)
            self.report.configure(state="disabled")

    def _progress(self, value: dict) -> None:
        if self.window and self.window.winfo_exists():
            rate = f" · {value['rate']:.0f} Dateien/s" if "rate" in value else ""
            total = f" / {value['total']}" if value.get("total") else ""
            self.progress_var.set(f"{value['phase']}: {value['count']}{total}{rate}")

    def _worker(self, root: Path, operation, callback=None) -> None:
        if self.busy or self._closed:
            return
        self.busy = True
        self.cancel.clear()

        def work():
            try:
                with CollectionIndex.open(root) as index:
                    result = operation(index)
                    status = index.status()
                self.dispatcher.post(self._done, result, status, callback)
            except Exception as exc:
                self.dispatcher.post(self._error, str(exc))

        threading.Thread(target=work, name="CollectionIndex", daemon=True).start()

    def _done(self, result, status, callback) -> None:
        self.busy = False
        if self.window and self.window.winfo_exists():
            last_scan = (
                datetime.fromisoformat(status["last_scan"])
                .astimezone()
                .strftime("%d.%m.%Y %H:%M")
                if status["last_scan"]
                else "–"
            )
            self.status_var.set(
                f"{status['files']} Dateien · {status['photos']} Fotos · {status['missing']} fehlend · "
                f"Metadaten {status['metadata']}/{status['files']} · letzter Schnellscan: {last_scan}"
                + (" · NUR LESBAR (gesperrt)" if status["readonly"] else "")
            )
        if callback:
            callback(result)
        if self.metadata_view and not self.metadata_view._closed:
            self.metadata_view.reload()

    def _error(self, error: str) -> None:
        self.busy = False
        self._show(error)
        messagebox.showerror("Sammlungsindex", error, parent=self.root)

    def refresh(self) -> None:
        root = self.target()
        if root and (root / ".myfotomanager").exists():
            self._worker(root, lambda index: None)

    def run(
        self,
        kind: str,
        scope: str = "",
        *,
        root: Path | None = None,
        allow_missing=False,
    ) -> None:
        root = root or self.target()
        if not root or self.busy:
            return
        if not (root / ".myfotomanager").exists():
            if not root.is_dir():
                messagebox.showerror(
                    "Sammlung", "Zielordner ist nicht erreichbar.", parent=self.root
                )
                return
            if not messagebox.askyesno(
                "Sammlung indizieren?",
                "Im Zielordner wird .myfotomanager angelegt.\nFotos werden nur gelesen und nicht verändert.",
                parent=self.root,
            ):
                return
        if str(root).startswith("\\\\") and not self.settings.get(
            "collection_network_ack", False
        ):
            if not messagebox.askyesno(
                "Netzwerksammlung",
                "SQLite-Sperren auf SMB/NAS können unzuverlässig sein. Nur von einem Rechner gleichzeitig schreiben. Fortfahren?",
                parent=self.root,
            ):
                return
            self.settings.set("collection_network_ack", True)
        self.last_root = root

        def operation(index):
            kwargs = dict(
                cancel=self.cancel,
                progress=lambda value: self.dispatcher.post(
                    self._progress, value, key="index-progress"
                ),
            )
            if kind != "enrich_metadata":
                kwargs["scope"] = scope
            if kind == "quick_scan":
                kwargs["confirm_missing"] = (
                    (lambda count, total: True) if allow_missing else None
                )
            result = getattr(index, kind)(**kwargs)
            changes = [
                change
                for change in index.changes_since(result.scan_id - 1)
                if change["scan_id"] == result.scan_id
            ]
            return result, changes

        def completed(payload):
            result, changes = payload
            self.last_result = result
            labels = {
                "added": "Neu",
                "modified": "Geändert",
                "moved": "Verschoben/umbenannt",
                "missing": "Fehlend",
                "returned": "Wieder aufgetaucht",
                "merged": "Fotos zusammengeführt",
            }
            state = {
                "done": "Abgeschlossen",
                "cancelled": "Abgebrochen",
                "error": "Fehler",
                "needs_confirmation": "Bestätigung erforderlich",
            }.get(result.status, result.status)
            lines = [
                state,
                f"{result.stats.get('count', result.stats.get('processed', 0))} Dateien geprüft",
            ]
            for item in result.stats.get("renamed_folders", []):
                lines.append(f"Ordner umbenannt: {item['old']} → {item['new']}")
            for item in result.stats.get("ambiguous", []):
                lines.append(
                    f"Mehrdeutig: {item['new']} ← {', '.join(item['candidates'])}"
                )
            for name in result.stats.get("mismatches", []):
                lines.append(f"Prüfsumme abweichend (möglicher Datenfehler): {name}")
            for name in result.stats.get("changed_since_scan", []):
                lines.append(f"Seit Schnellscan geändert, bitte erneut scannen: {name}")
            if result.stats.get("uncertain"):
                lines.append(
                    f"{result.stats['uncertain']} kürzlich geänderte Dateien: beim nächsten Scan erneut prüfen"
                )
            if result.stats.get("missing_skipped"):
                lines.append(
                    f"{result.stats['missing_skipped']} fehlende Einträge aus Sicherheitsgründen beibehalten"
                )
            lines.extend(result.errors)
            for key, label in labels.items():
                items = [change for change in changes if change["kind"] == key]
                lines.append(f"\n{label}: {len(items)}")
                lines.extend(
                    f"{item['old_relpath'] or ''} → {item['new_relpath'] or ''}"
                    for item in items
                )
            self._show("\n".join(lines))
            if self.window and self.window.winfo_exists():
                self.progress_var.set(state)
            if result.status == "needs_confirmation" and messagebox.askyesno(
                "Viele fehlende Dateien",
                f"{result.stats['missing_skipped']} Dateien fehlen (über 20 %).\nLaufwerk angeschlossen? Erneut prüfen und fehlend markieren?",
                parent=self.root,
            ):
                self.run(kind, scope, root=root, allow_missing=True)

        self._worker(root, operation, completed)

    def scan_folder(self) -> None:
        root = self.target()
        if not root:
            return
        chosen = filedialog.askdirectory(parent=self.root, initialdir=root)
        if chosen:
            try:
                scope = Path(chosen).resolve().relative_to(root.resolve()).as_posix()
            except ValueError:
                messagebox.showerror(
                    "Sammlung", "Ordner muss in der Sammlung liegen.", parent=self.root
                )
                return
            self.run("quick_scan", "" if scope == "." else scope)

    def purge(self) -> None:
        root = self.target()
        if root and messagebox.askyesno(
            "Fehlende bereinigen?",
            "Fehlende Dateieinträge und ihre EXIF-Daten endgültig entfernen?\nStabile Foto-IDs und spätere Nutzerdaten (z. B. Gesichter) bleiben erhalten.",
            parent=self.root,
        ):
            self._worker(
                root,
                lambda index: index.purge_missing(),
                lambda count: self._show(f"{count} fehlende Dateieinträge entfernt."),
            )

    def options(self) -> None:
        root = self.target()
        if not root or not (root / ".myfotomanager").exists():
            return

        def edit(settings):
            dialog = tk.Toplevel(self.root)
            dialog.title("Index-Optionen")
            pane = ttk.Frame(dialog, padding=16)
            pane.pack(fill="both", expand=True)
            ttk.Label(pane, text="Mehrere Werte mit ; trennen").grid(
                row=0, columnspan=2, sticky="w"
            )
            fields = {}
            for number, (key, label) in enumerate(
                (
                    ("extensions", "Dateiendungen"),
                    ("exclusions", "Ausschlüsse (z. B. Privat/*)"),
                    ("export_folders", "Eigenständige Export-Ordner"),
                    ("extension_folders", "Endungsordner für RAW/JPG-Paare"),
                    ("exiftool", "ExifTool-Programm / Pfad"),
                ),
                1,
            ):
                value = settings[key]
                fields[key] = tk.StringVar(
                    value="; ".join(value) if isinstance(value, list) else value
                )
                ttk.Label(pane, text=label).grid(
                    row=number, column=0, sticky="w", padx=(0, 12), pady=6
                )
                ttk.Entry(pane, textvariable=fields[key], width=65).grid(
                    row=number, column=1, sticky="ew"
                )

            def save():
                if self.busy:
                    return
                values = {
                    key: variable.get().strip()
                    if key == "exiftool"
                    else [
                        part.strip()
                        for part in variable.get().split(";")
                        if part.strip()
                    ]
                    for key, variable in fields.items()
                }
                values["extensions"] = [
                    part.lstrip(".").lower() for part in values["extensions"]
                ]
                dialog.destroy()
                self._worker(root, lambda index: index.configure(**values))

            ttk.Button(pane, text="Speichern", command=save).grid(
                row=6, column=1, sticky="e", pady=12
            )

        self._worker(root, lambda index: index.settings, edit)

    def resolve(self) -> None:
        if not self.last_result or not self.last_root:
            return
        candidates = self.last_result.stats.get("ambiguous", [])
        if not candidates:
            return
        new = simpledialog.askstring(
            "Verschiebung auflösen",
            "Neuer relativer Pfad (siehe Bericht)",
            parent=self.root,
        )
        old = (
            simpledialog.askstring(
                "Verschiebung auflösen",
                "Bisheriger relativer Pfad (siehe Bericht)",
                parent=self.root,
            )
            if new
            else None
        )
        if old and any(
            item["new"] == new and old in item["candidates"] for item in candidates
        ):
            self._worker(
                self.last_root,
                lambda index: index.resolve_move(old, new),
                lambda _: self._show(
                    "Verschiebung zugeordnet; bisherige Foto-ID bleibt erhalten."
                ),
            )

    def check_import(self, root: Path, files: list[Path], callback) -> None:
        """Check before copying; errors leave the import pending for review."""
        if not (root / ".myfotomanager").exists():
            callback(files)
            return
        if self.busy:
            messagebox.showinfo(
                "Sammlungsindex",
                "Index beschäftigt. Bitte den Import nach dem Scan erneut starten.",
                parent=self.root,
            )
            return

        def checked(statuses):
            already = {
                item.card_path
                for item in statuses
                if item.status in ("imported", "imported_likely")
            }
            if self.on_import_status:
                self.on_import_status(statuses)
            callback(
                [path for path in files if path not in already]
                if self.settings.get("collection_skip_imported", True)
                else files
            )

        self._worker(root, lambda index: index.check_import(files), checked)

    def inspect_import(self, root: Path, files: list[Path], callback) -> None:
        """Annotate source days in the background without blocking source browsing."""
        self._inspect_revision += 1
        revision = self._inspect_revision
        if not (root / ".myfotomanager").exists():
            return

        def work():
            try:
                with CollectionIndex.open(root, readonly=True) as index:
                    result = index.check_import(files)
                self.dispatcher.post(deliver, result)
            except Exception as exc:
                self.dispatcher.post(
                    self._show, f"Import-Prüfung noch nicht verfügbar: {exc}"
                )

        def deliver(result):
            if revision == self._inspect_revision:
                callback(result)

        threading.Thread(target=work, name="CollectionImportCheck", daemon=True).start()

    def after_copy(self, root: Path, folder: Path) -> None:
        if (root / ".myfotomanager").exists():
            if self.busy:
                self.root.after(
                    500,
                    lambda: self.after_copy(root, folder) if not self._closed else None,
                )
            else:
                self.run("quick_scan", folder.relative_to(root).as_posix(), root=root)

    def close(self) -> None:
        self._closed = True
        self.cancel.set()
        self.root.after_cancel(self._night_timer)
        self.root.after_cancel(self._offer_timer)
        self.dispatcher.close()
        if self.metadata_view:
            self.metadata_view.close()
