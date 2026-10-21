"""Three-column import workspace and modeless application settings."""

from __future__ import annotations

import tkinter as tk
import webbrowser
from pathlib import Path
from tkinter import filedialog, ttk
from collections.abc import Callable

from config.settings import DNG_CONVERTER_PATH, DNG_DOWNLOAD_URL, Settings
from ui.date_list_widget import DateListWidget
from ui.folder_selector import FolderSelector
from ui.preview_widget import PreviewWidget
from ui.progress_manager import ProgressManager
from ui.scan_progress import ScanProgressWidget
from ui.theme import configure_theme
from ui.scroll_frame import ScrollFrame


class MainLayout(ttk.Frame):
    """Expose controls to the controller without doing filesystem work."""

    def __init__(
        self,
        root: tk.Tk,
        settings: Settings,
        *,
        on_source_selected: Callable[[Path], None],
        on_target_selected: Callable[[Path], None],
        on_preview_count_changed: Callable[[], None],
        on_date_source_changed: Callable[[], None],
        on_date_selection_changed: Callable[[set[str]], None],
        on_custom_name_changed: Callable[[], None],
        on_execute_copy: Callable[[], None],
    ) -> None:
        configure_theme(root)
        super().__init__(root, padding=12, style="Canvas.TFrame")
        self.grid(row=0, column=0, sticky="nsew")
        root.columnconfigure(0, weight=1)
        root.rowconfigure(0, weight=1)
        root.minsize(1080, 720)
        self.columnconfigure(0, weight=1)
        self.rowconfigure(2, weight=1)
        self.settings = settings
        self.preview_count_var = tk.IntVar(value=settings.preview_count)
        self.date_source_var = tk.StringVar(value=settings.date_source)
        self.custom_name_var = tk.StringVar()
        self.convert_to_dng_var = tk.BooleanVar(
            value=settings.get("convert_to_dng", False)
        )
        self.dng_converter_var = tk.StringVar(
            value=settings.get("dng_converter_path", DNG_CONVERTER_PATH)
        )

        header = ttk.Frame(self, padding=(16, 8, 16, 14))
        header.grid(row=0, column=0, sticky="ew")
        ttk.Label(header, text="Fotos importieren", style="Title.TLabel").pack(
            anchor="w"
        )
        ttk.Label(
            header, text="Auswählen. Benennen. Importieren.", style="Muted.TLabel"
        ).pack(anchor="w")
        ttk.Button(
            header,
            text="⚙  Einstellungen",
            style="Link.TButton",
            command=self.open_settings,
        ).place(relx=1, rely=0.5, anchor="e")

        source = ttk.Frame(self, padding=(14, 8))
        source.grid(row=1, column=0, sticky="ew", pady=(0, 8))
        source.columnconfigure(0, weight=3)
        source.columnconfigure(1, weight=2)

        body = ttk.Panedwindow(self, orient="horizontal")
        body.grid(row=2, column=0, sticky="nsew")
        self.date_list = DateListWidget(body, on_date_selection_changed)
        body.add(self.date_list, weight=0)
        center = ttk.Frame(body, padding=16)
        body.add(center, weight=3)
        right = ttk.Frame(body, padding=16, width=340)
        body.add(right, weight=0)
        right.columnconfigure(0, weight=1)
        right.rowconfigure(0, weight=1)
        inspector_scroll = ScrollFrame(right)
        inspector_scroll.grid(row=0, column=0, sticky="nsew")
        inspector = inspector_scroll.content
        actions = ttk.Frame(right)
        actions.grid(row=1, column=0, sticky="ew")
        actions.columnconfigure(0, weight=1)

        self.folder_selector = FolderSelector(
            source,
            on_source_selected,
            on_target_selected,
            settings.last_source_folder,
            settings.last_target_folder,
            target_parent=inspector,
        )
        self.folder_selector.grid(row=0, column=0, sticky="w")
        self.scan_progress = ScanProgressWidget(source)
        self.scan_progress.grid(row=0, column=1, sticky="ew", padx=(20, 0))

        center.columnconfigure(0, weight=1)
        center.rowconfigure(3, weight=1)
        self.preview_title = ttk.Label(
            center, text="Deine Aufnahmen", style="Heading.TLabel"
        )
        self.preview_title.grid(row=0, column=0, sticky="w")
        self.preview_details = ttk.Label(
            center, text="Links einen Aufnahmetag auswählen", style="Muted.TLabel"
        )
        self.preview_details.grid(row=1, column=0, sticky="w", pady=(4, 12))
        self.preview_caption = ttk.Label(
            center, text="Bildvorschau", style="Small.TLabel"
        )
        self.preview_caption.grid(row=2, column=0, sticky="w", pady=(0, 8))
        self.preview_widget = PreviewWidget(center, thumbnail_size=(240, 160))
        self.preview_widget.grid(row=3, column=0, sticky="nsew")
        self.preview_widget.load_previews([])
        zoom = ttk.Frame(center)
        zoom.grid(row=2, column=0, sticky="e", pady=(0, 8))
        ttk.Label(zoom, text="Größe", style="Small.TLabel").pack(side="left", padx=6)
        self._zoom_after = None

        def zoom_changed(value):
            if self._zoom_after:
                self.after_cancel(self._zoom_after)
            self._zoom_after = self.after(
                180, lambda: self._apply_zoom(int(float(value)))
            )

        self.zoom = ttk.Scale(zoom, from_=140, to=300, length=85, command=zoom_changed)
        self.zoom.set(240)
        self.zoom.pack(side="left")

        footer = self.date_list.footer
        ttk.Separator(footer).pack(fill="x", pady=(0, 10))
        ttk.Label(footer, text="Gruppierung", style="Small.TLabel").pack(anchor="w")
        group_var = tk.StringVar(
            value="Foto-Metadaten"
            if settings.date_source == "metadata"
            else "Dateisystem-Zeitstempel"
        )
        combo = ttk.Combobox(
            footer,
            textvariable=group_var,
            state="readonly",
            values=("Foto-Metadaten", "Dateisystem-Zeitstempel"),
            width=27,
        )
        combo.pack(fill="x", pady=(5, 0))

        def grouping_changed(event=None):
            self.date_source_var.set(
                "metadata" if group_var.get() == "Foto-Metadaten" else "filesystem"
            )
            on_date_source_changed()

        combo.bind("<<ComboboxSelected>>", grouping_changed)

        # Target controls are placed by FolderSelector inside this inspector.
        ttk.Label(inspector, text="Import vorbereiten", style="Heading.TLabel").grid(
            row=0, column=0, columnspan=2, sticky="w", pady=(0, 10)
        )
        inspector.columnconfigure(0, weight=1)
        ttk.Label(inspector, text="Ordnername").grid(
            row=3, column=0, columnspan=2, sticky="w", pady=(8, 5)
        )
        self.custom_name_entry = ttk.Entry(
            inspector, textvariable=self.custom_name_var, width=30
        )
        self.custom_name_entry.grid(row=4, column=0, columnspan=2, sticky="ew")
        self.custom_name_entry.bind("<Return>", lambda event: on_execute_copy())
        ttk.Separator(inspector).grid(
            row=5, column=0, columnspan=2, sticky="ew", pady=10
        )
        ttk.Label(inspector, text="So wird gespeichert").grid(
            row=6, column=0, columnspan=2, sticky="w"
        )
        self.destination_tree = ttk.Treeview(
            inspector,
            show="tree",
            height=4,
            selectmode="none",
            style="Destination.Treeview",
        )
        ttk.Style(root).configure("Destination.Treeview", rowheight=23)
        self.destination_tree.column("#0", width=280, stretch=True)
        self.destination_tree.grid(
            row=7, column=0, columnspan=2, sticky="ew", pady=(8, 0)
        )
        ttk.Label(
            inspector, text="Unterordner nach Dateityp", style="Small.TLabel"
        ).grid(row=8, column=0, columnspan=2, sticky="w", pady=(6, 0))
        ttk.Separator(inspector).grid(
            row=9, column=0, columnspan=2, sticky="ew", pady=10
        )
        ttk.Label(inspector, text="Konvertierung").grid(row=10, column=0, sticky="w")
        ttk.Checkbutton(
            inspector,
            text="NEF zusätzlich als DNG",
            variable=self.convert_to_dng_var,
            command=lambda: (
                settings.set("convert_to_dng", self.convert_to_dng_var.get()),
                on_custom_name_changed(),
            ),
        ).grid(row=11, column=0, columnspan=2, sticky="w", pady=(8, 4))
        ttk.Label(
            inspector, text="RAW-Originale bleiben erhalten.", style="Small.TLabel"
        ).grid(row=12, column=0, columnspan=2, sticky="w")
        ttk.Button(
            inspector,
            text="Konfigurieren",
            style="Link.TButton",
            command=self.open_settings,
        ).grid(row=13, column=0, columnspan=2, sticky="w", pady=(5, 0))
        self.converter_status = ttk.Label(
            inspector, text="Wird geprüft …", style="Small.TLabel", wraplength=165
        )
        self.converter_status.grid(row=10, column=1, sticky="e")
        self.selection_summary = ttk.Label(
            actions, text="Kein Tag ausgewählt", font=("Segoe UI", 11, "bold")
        )
        self.selection_summary.grid(row=0, column=0, sticky="w", pady=(14, 8))
        self.execute_button = ttk.Button(
            actions,
            text="Dateien importieren →",
            style="Primary.TButton",
            command=on_execute_copy,
            state="disabled",
        )
        self.execute_button.grid(row=1, column=0, sticky="ew")
        self.import_hint = ttk.Label(
            actions,
            text="Dateien werden kopiert.",
            style="Small.TLabel",
            wraplength=290,
        )
        self.import_hint.grid(row=2, column=0, sticky="w", pady=(8, 0))
        inspector_scroll.bind_wheel()

        jobs = ttk.Frame(self, padding=(16, 10))
        jobs.grid(row=3, column=0, sticky="ew", pady=(8, 0))
        self.progress_manager = ProgressManager(jobs)
        self.status_label = ttk.Label(
            self, text="Bereit", style="Small.TLabel", padding=(4, 5)
        )
        self.status_label.grid(row=4, column=0, sticky="ew")

        # Keep settings widgets alive while the modeless window is hidden.
        self.settings_window = tk.Toplevel(root)
        self.settings_window.withdraw()
        self.settings_window.title("Einstellungen · Foto-Importer")
        self.settings_window.protocol("WM_DELETE_WINDOW", self.settings_window.withdraw)
        options = ttk.Frame(self.settings_window, padding=24)
        options.pack(fill="both", expand=True)
        options.columnconfigure(1, weight=1)
        ttk.Label(options, text="Einstellungen", style="Heading.TLabel").grid(
            row=0, column=0, columnspan=3, sticky="w", pady=(0, 10)
        )
        ttk.Label(options, text="Anzahl Vorschaubilder").grid(
            row=1, column=0, sticky="w"
        )
        self.preview_spinbox = ttk.Spinbox(
            options,
            from_=1,
            to=50,
            width=6,
            textvariable=self.preview_count_var,
            command=on_preview_count_changed,
        )
        self.preview_spinbox.grid(row=1, column=1, sticky="w", padx=12)
        self.preview_spinbox.bind("<Return>", lambda e: on_preview_count_changed())
        self.preview_spinbox.bind("<FocusOut>", lambda e: on_preview_count_changed())
        ttk.Label(options, text="Adobe DNG Converter").grid(
            row=2, column=0, sticky="w", pady=(24, 8)
        )
        entry = ttk.Entry(options, textvariable=self.dng_converter_var, width=60)
        entry.grid(row=3, column=0, columnspan=2, sticky="ew")
        entry.bind(
            "<FocusOut>",
            lambda e: settings.set("dng_converter_path", self.dng_converter_var.get()),
        )

        def choose_converter():
            path = filedialog.askopenfilename(
                parent=self.settings_window,
                title="Adobe DNG Converter auswählen",
                filetypes=[("Programm", "*.exe")],
            )
            if path:
                self.dng_converter_var.set(path)
                settings.set("dng_converter_path", path)

        ttk.Button(options, text="Auswählen …", command=choose_converter).grid(
            row=3, column=2, padx=(10, 0)
        )
        self.dng_version_label = ttk.Label(
            options, text="Converter wird geprüft …", wraplength=620
        )
        self.dng_version_label.grid(row=4, column=0, columnspan=3, sticky="w", pady=12)
        self.dng_download_button = ttk.Button(
            options,
            text="Update herunterladen",
            state="disabled",
            command=lambda: webbrowser.open(DNG_DOWNLOAD_URL),
        )
        self.dng_download_button.grid(row=5, column=0, sticky="w")
        ttk.Button(
            options, text="Schließen", command=self.settings_window.withdraw
        ).grid(row=6, column=2, pady=(20, 0))
        self.custom_name_var.trace_add("write", lambda *args: on_custom_name_changed())

    def _apply_zoom(self, width: int) -> None:
        self._zoom_after = None
        self.preview_widget.set_thumbnail_size(width)

    def destroy(self) -> None:
        if self._zoom_after:
            self.after_cancel(self._zoom_after)
        super().destroy()

    def open_settings(self) -> None:
        """Show settings without blocking imports or grabbing the main window."""
        self.settings_window.deiconify()
        self.settings_window.lift()

    def update_destination(self, counts: dict[str, int]) -> None:
        """Render the exact extension folders and root-level generated DNGs."""
        self.destination_tree.delete(*self.destination_tree.get_children())
        name = self.custom_name_var.get().strip() or "Ordnername eingeben"
        self.destination_tree.insert(
            "", "end", iid="root", text="📁  " + name, open=True
        )
        for extension, count in sorted(counts.items()):
            self.destination_tree.insert(
                "root", "end", text=f"{extension}   ·   {count} Dateien"
            )
        if self.convert_to_dng_var.get() and counts.get("nef"):
            self.destination_tree.insert(
                "root", "end", text=f"{counts['nef']} DNG direkt in diesem Ordner"
            )
