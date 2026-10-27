"""Collection metadata dashboard with bounded, asynchronous browser requests."""

from __future__ import annotations

import os
import subprocess
import sys
import tkinter as tk
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from fractions import Fraction
from pathlib import Path
from tkinter import ttk

from PIL import ImageTk

from config.settings import METADATA_PAGE_SIZE, METADATA_SEARCH_DELAY_MS
from core.metadata_browser import (
    MetadataFilter,
    load_metadata_detail,
    load_metadata_page,
)
from ui.dispatcher import UiDispatcher
from ui.scroll_frame import ScrollFrame
from ui.theme import ACCENT, MUTED, SURFACE, TEXT


def display_date(value: str | None) -> str:
    if not value:
        return "–"
    try:
        return datetime.strptime(value[:19], "%Y:%m:%d %H:%M:%S").strftime(
            "%d.%m.%Y %H:%M:%S"
        )
    except ValueError:
        return value


def exposure_text(value) -> str:
    if value in (None, ""):
        return "–"
    try:
        number = float(value)
        return (
            f"{Fraction(number).limit_denominator(100000)} s"
            if 0 < number < 1
            else f"{number:g} s"
        )
    except (ValueError, TypeError):
        return f"{value} s"


class MetadataChart(tk.Canvas):
    """Draw small responsive camera bars and ISO bins without chart dependencies."""

    def __init__(self, parent: tk.Misc, *, histogram: bool = False) -> None:
        super().__init__(
            parent, background=SURFACE, highlightthickness=0, height=175, width=240
        )
        self.histogram = histogram
        self.values = []
        self.bind("<Configure>", lambda _: self.draw())

    def set_values(self, values) -> None:
        self.values = values
        self.draw()

    def draw(self) -> None:
        self.delete("all")
        width, height = max(self.winfo_width(), 240), max(self.winfo_height(), 180)
        if not self.values or not any(value for _, value in self.values):
            self.create_text(
                width / 2,
                height / 2,
                text="Noch keine Metadaten",
                fill=MUTED,
                font=("Segoe UI", 10),
            )
            return
        largest = max(value for _, value in self.values)
        if self.histogram:
            left, bottom, top = 42, height - 30, 18
            available = width - left - 14
            for step in range(4):
                y = bottom - (bottom - top) * step / 3
                self.create_line(left, y, width - 10, y, fill="#e9edf2")
                self.create_text(
                    left - 7,
                    y,
                    anchor="e",
                    text=f"{largest * step / 3:,.0f}".replace(",", "."),
                    fill=MUTED,
                    font=("Segoe UI", 9),
                )
            slot = available / len(self.values)
            for position, (label, value) in enumerate(self.values):
                x = left + slot * position + slot * 0.22
                y = bottom - (bottom - top) * value / largest
                self.create_rectangle(
                    x, y, x + slot * 0.56, bottom, fill="#79afea", outline=""
                )
                self.create_text(
                    x + slot * 0.28,
                    bottom + 16,
                    text=label,
                    fill=MUTED,
                    font=("Segoe UI", 9),
                )
        else:
            label_width = min(130, width * 0.4)
            bar_width = max(30, width - label_width - 62)
            slot = (height - 20) / max(3, len(self.values))
            for position, (label, value) in enumerate(self.values):
                y = 12 + slot * position
                short = label if len(label) <= 19 else label[:17] + "…"
                self.create_text(
                    0, y + 13, text=short, anchor="w", fill=TEXT, font=("Segoe UI", 10)
                )
                end = label_width + bar_width * value / largest
                self.create_rectangle(
                    label_width, y + 2, end, y + 25, fill=ACCENT, outline=""
                )
                self.create_text(
                    end + 7,
                    y + 13,
                    text=f"{value:,}".replace(",", "."),
                    anchor="w",
                    fill=TEXT,
                    font=("Segoe UI", 9),
                )


class MetadataView(ttk.Frame):
    """Query one page at a time, ignoring outdated filter and selection results."""

    def __init__(
        self,
        parent: tk.Misc,
        target: Callable[[], Path | None],
        enrich: Callable[[], None],
    ) -> None:
        super().__init__(parent, padding=14, style="Canvas.TFrame")
        self.target = target
        self._closed = False
        self._generation = 0
        self._detail_generation = 0
        self._debounce = None
        self._future = self._detail_future = None
        self._query_pool = ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="MetadataQuery"
        )
        self._detail_pool = ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="MetadataDetail"
        )
        self.dispatcher = UiDispatcher(self)
        self.rows = {}
        self.page = 0
        self.total = 0
        self.sort = "date"
        self.descending = True
        self.selected_detail = None
        self.preview_image = None
        self._resize_timer = None
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)
        toolbar = ttk.Frame(self, padding=(10, 4, 10, 10))
        toolbar.grid(row=0, column=0, sticky="ew")
        ttk.Label(toolbar, text="Metadaten deiner Fotos", style="Heading.TLabel").pack(
            side="left"
        )
        ttk.Button(toolbar, text="Metadaten ergänzen", command=enrich).pack(
            side="right"
        )
        ttk.Button(toolbar, text="Aktualisieren", command=self.reload).pack(
            side="right", padx=6
        )
        body = ttk.Panedwindow(self, orient="horizontal")
        body.grid(row=1, column=0, sticky="nsew")
        left = ttk.Frame(body, padding=12)
        right = ttk.Frame(body, padding=12, width=420)
        body.add(left, weight=3)
        body.add(right, weight=2)
        left.columnconfigure(0, weight=1)
        left.rowconfigure(3, weight=1)
        stats = ttk.Frame(left)
        stats.grid(row=0, column=0, sticky="ew", pady=(0, 18))
        self.summary_vars = []
        for column, label in enumerate(
            ("Fotos", "mit Metadaten", "Kameras", "Objektive")
        ):
            stats.columnconfigure(column, weight=1)
            tile = ttk.Frame(stats, padding=8)
            tile.grid(row=0, column=column, sticky="ew")
            variable = tk.StringVar(value="–")
            self.summary_vars.append(variable)
            ttk.Label(
                tile, textvariable=variable, style="Heading.TLabel", anchor="center"
            ).pack(fill="x")
            ttk.Label(tile, text=label, style="Muted.TLabel", anchor="center").pack(
                fill="x"
            )
        filters = ttk.Frame(left)
        filters.grid(row=1, column=0, sticky="ew", pady=(0, 14))
        filters.columnconfigure(0, weight=1)
        self.search = tk.StringVar()
        self.camera = tk.StringVar(value="Alle Kameras")
        self.year = tk.StringVar(value="Alle Jahre")
        self.search_entry = ttk.Entry(filters, textvariable=self.search)
        self.search_entry.grid(row=1, column=0, sticky="ew", padx=(0, 8))
        ttk.Label(
            filters, text="Dateiname, Kamera oder Objektiv", style="Small.TLabel"
        ).grid(row=0, column=0, sticky="w")
        self.camera_combo = ttk.Combobox(
            filters,
            textvariable=self.camera,
            values=("Alle Kameras",),
            state="readonly",
            width=18,
        )
        self.camera_combo.grid(row=1, column=1, padx=(0, 8))
        self.year_combo = ttk.Combobox(
            filters,
            textvariable=self.year,
            values=("Alle Jahre",),
            state="readonly",
            width=11,
        )
        self.year_combo.grid(row=1, column=2)
        self.search.trace_add("write", self._filter_changed)
        self.camera_combo.bind("<<ComboboxSelected>>", self._filter_changed)
        self.year_combo.bind("<<ComboboxSelected>>", self._filter_changed)
        charts = ttk.Frame(left)
        charts.grid(row=2, column=0, sticky="ew", pady=(0, 14))
        charts.columnconfigure((0, 1), weight=1, uniform="charts")
        for column, (title, subtitle, attr, histogram) in enumerate(
            (
                (
                    "Fotos nach Kamera",
                    "Bis zu fünf Kameras · aktuelle Auswahl",
                    "camera_chart",
                    False,
                ),
                ("ISO-Verteilung", "Fotos mit ISO-Angabe", "iso_chart", True),
            )
        ):
            frame = ttk.Frame(charts, padding=(6, 6))
            frame.grid(row=0, column=column, sticky="nsew")
            ttk.Label(frame, text=title, style="Heading.TLabel").pack(anchor="w")
            ttk.Label(frame, text=subtitle, style="Small.TLabel").pack(anchor="w")
            chart = MetadataChart(frame, histogram=histogram)
            chart.pack(fill="both", expand=True)
            setattr(self, attr, chart)
        table = ttk.Frame(left)
        table.grid(row=3, column=0, sticky="nsew")
        table.columnconfigure(0, weight=1)
        table.rowconfigure(1, weight=1)
        ttk.Label(table, text="Fotos", style="Heading.TLabel").grid(
            row=0, column=0, sticky="w", pady=(0, 8)
        )
        style = ttk.Style(self)
        style.configure("Metadata.Treeview", rowheight=34)
        self.tree = ttk.Treeview(
            table,
            columns=("name", "date", "camera", "iso"),
            show="headings",
            selectmode="browse",
            height=6,
            style="Metadata.Treeview",
        )
        for key, label, width in (
            ("name", "Datei", 185),
            ("date", "Aufgenommen", 160),
            ("camera", "Kamera", 135),
            ("iso", "ISO", 60),
        ):
            self.tree.heading(key, text=label, command=lambda key=key: self._sort(key))
            self.tree.column(key, width=width, minwidth=45, stretch=key != "iso")
        self.tree.grid(row=1, column=0, sticky="nsew")
        scroll = ttk.Scrollbar(table, command=self.tree.yview)
        scroll.grid(row=1, column=1, sticky="ns")
        self.tree.configure(yscrollcommand=scroll.set)
        self.tree.bind("<<TreeviewSelect>>", self._selected)
        pages = ttk.Frame(left)
        pages.grid(row=4, column=0, sticky="ew", pady=(10, 0))
        self.selection_var = tk.StringVar(value="Kein Foto ausgewählt")
        ttk.Label(pages, textvariable=self.selection_var, style="Small.TLabel").pack(
            side="left"
        )
        self.next_button = ttk.Button(
            pages, text="Weiter →", command=lambda: self._page(1)
        )
        self.next_button.pack(side="right")
        self.previous_button = ttk.Button(
            pages, text="← Zurück", command=lambda: self._page(-1)
        )
        self.previous_button.pack(side="right", padx=4)
        right.columnconfigure(0, weight=1)
        right.rowconfigure(0, weight=1)
        inspector = ScrollFrame(right)
        self.inspector = inspector
        inspector.grid(row=0, column=0, sticky="nsew")
        inspector.bind("<Configure>", self._preview_resized)
        details = inspector.content
        self.name_var = tk.StringVar(value="Foto auswählen")
        ttk.Label(
            details, textvariable=self.name_var, style="Heading.TLabel", wraplength=390
        ).pack(anchor="w", pady=(0, 10))
        self.preview = ttk.Label(
            details,
            text="Die Bildvorschau erscheint hier.",
            anchor="center",
            background="#edf1f5",
        )
        self.preview.pack(fill="x")
        self.preview.bind("<Configure>", self._preview_resized)
        self.exposure_var = tk.StringVar(value="–     –     –     –")
        ttk.Label(
            details,
            textvariable=self.exposure_var,
            font=("Segoe UI", 12, "bold"),
            anchor="center",
        ).pack(fill="x", pady=14)
        self.detail_vars = {}
        for title, fields in (
            (
                "Aufnahme",
                (("dt_original", "Datum"), ("model", "Kamera"), ("lens", "Objektiv")),
            ),
            (
                "Datei",
                (
                    ("resolution", "Auflösung"),
                    ("size", "Größe"),
                    ("rating", "Bewertung"),
                ),
            ),
        ):
            ttk.Label(details, text=title, style="Muted.TLabel").pack(
                anchor="w", pady=(10, 4)
            )
            ttk.Separator(details).pack(fill="x", pady=(0, 6))
            group = ttk.Frame(details)
            group.pack(fill="x")
            group.columnconfigure(1, weight=1)
            for number, (key, label) in enumerate(fields):
                ttk.Label(group, text=label, style="Muted.TLabel").grid(
                    row=number, column=0, sticky="nw", padx=(0, 14), pady=3
                )
                variable = self.detail_vars[key] = tk.StringVar(value="–")
                ttk.Label(
                    group,
                    textvariable=variable,
                    wraplength=270,
                    foreground=ACCENT if key == "rating" else TEXT,
                ).grid(row=number, column=1, sticky="w", pady=3)
        ttk.Separator(details).pack(fill="x", pady=12)
        self.extra_button = ttk.Button(
            details,
            text="▸ Weitere Metadaten",
            style="Link.TButton",
            command=self._toggle_extra,
        )
        self.extra_button.pack(anchor="w")
        self.extra = ttk.Frame(details)
        self.extra_var = tk.StringVar(value="–")
        ttk.Label(
            self.extra, textvariable=self.extra_var, wraplength=360, justify="left"
        ).pack(anchor="w", pady=8)
        self.reveal_button = ttk.Button(
            right, text="Datei im Ordner zeigen", command=self.reveal, state="disabled"
        )
        self.reveal_button.grid(row=1, column=0, sticky="ew", pady=(10, 0))
        inspector.bind_wheel()
        self.footer = tk.StringVar(value="Sammlung wird geladen …")
        footer = ttk.Frame(self, padding=(10, 8))
        footer.grid(row=2, column=0, sticky="ew", pady=(8, 0))
        ttk.Label(footer, textvariable=self.footer, style="Small.TLabel").pack(
            side="left"
        )
        ttk.Label(footer, text="Dateien werden nur gelesen", style="Small.TLabel").pack(
            side="right"
        )
        self.reload()

    def _filter_changed(self, *args) -> None:
        self.page = 0
        # Invalidate immediately so an old result cannot flash during debounce.
        self._generation += 1
        self._clear_detail()
        if self._debounce:
            self.after_cancel(self._debounce)
        self._debounce = self.after(METADATA_SEARCH_DELAY_MS, self.reload)

    def reload(self) -> None:
        if self._closed:
            return
        if self._debounce:
            self.after_cancel(self._debounce)
            self._debounce = None
        self._generation += 1
        generation, root = self._generation, self.target()
        self._clear_detail()
        if self._future:
            self._future.cancel()
        self.footer.set("Metadaten werden geladen …")
        filters = MetadataFilter(
            self.search.get(),
            "" if self.camera.get() == "Alle Kameras" else self.camera.get(),
            "" if self.year.get() == "Alle Jahre" else self.year.get(),
            self.sort,
            self.descending,
            self.page,
        )

        def work():
            try:
                if root is None:
                    raise ValueError("Bitte zuerst einen Zielordner auswählen.")
                result = load_metadata_page(root, filters)
                self.dispatcher.post(self._loaded, generation, root, result)
            except Exception as exc:
                self.dispatcher.post(self._failed, generation, str(exc))

        self._future = self._query_pool.submit(work)

    def _loaded(self, generation, root, result) -> None:
        if self._closed or generation != self._generation or root != self.target():
            return
        self.tree.delete(*self.tree.get_children())
        self.rows = {str(row["photo_id"]): row for row in result["rows"]}
        for key, row in self.rows.items():
            self.tree.insert(
                "",
                "end",
                iid=key,
                values=(
                    row["name"],
                    display_date(row["dt_original"]),
                    row["camera"] or "–",
                    row["iso"] if row["iso"] is not None else "–",
                ),
            )
        summary = result["summary"]
        self.total, self.page = summary["total"], result["page"]
        values = (
            f"{self.total:,}".replace(",", "."),
            f"{round(100 * summary['metadata'] / self.total) if self.total else 0} %",
            str(summary["cameras"]),
            str(summary["lenses"]),
        )
        for variable, value in zip(self.summary_vars, values):
            variable.set(value)
        self.camera_combo.configure(values=["Alle Kameras", *result["cameras"]])
        self.year_combo.configure(values=["Alle Jahre", *result["years"]])
        self.camera_chart.set_values(result["camera_counts"])
        self.iso_chart.set_values(
            list(
                zip(
                    ("≤100", "≤200", "≤400", "≤800", "≤1600", ">1600"),
                    result["iso_counts"],
                )
            )
        )
        self.previous_button.configure(state="normal" if self.page else "disabled")
        self.next_button.configure(
            state="normal"
            if (self.page + 1) * METADATA_PAGE_SIZE < self.total
            else "disabled"
        )
        last = result["last_metadata"]
        stamp = (
            datetime.fromisoformat(last).astimezone().strftime("%d.%m.%Y %H:%M")
            if last
            else "noch nicht ergänzt"
        )
        self.footer.set(
            f"Seite {self.page + 1} · {self.total} Fotos · Metadaten: {stamp}"
            if self.total
            else (
                "Keine Fotos für diese Filter."
                if result["indexed"]
                else "Noch kein Index. Im Reiter Index zuerst Änderungen suchen."
            )
        )
        if self.rows:
            first = next(iter(self.rows))
            self.tree.selection_set(first)
            self.tree.focus(first)

    def _failed(self, generation, message) -> None:
        if not self._closed and generation == self._generation:
            self.tree.delete(*self.tree.get_children())
            self.rows = {}
            self.total = 0
            for variable in self.summary_vars:
                variable.set("–")
            self.camera_chart.set_values([])
            self.iso_chart.set_values([])
            self.previous_button.configure(state="disabled")
            self.next_button.configure(state="disabled")
            self.footer.set(message)

    def _sort(self, key) -> None:
        self.descending = not self.descending if key == self.sort else False
        self.sort, self.page = key, 0
        for column, label in (
            ("name", "Datei"),
            ("date", "Aufgenommen"),
            ("camera", "Kamera"),
            ("iso", "ISO"),
        ):
            self.tree.heading(
                column,
                text=label
                + ((" ↓" if self.descending else " ↑") if column == key else ""),
            )
        self.reload()

    def _page(self, direction) -> None:
        self.page = max(0, self.page + direction)
        self.reload()

    def _clear_detail(self) -> None:
        self._detail_generation += 1
        if self._detail_future:
            self._detail_future.cancel()
        self.selected_detail = None
        self.preview_image = None
        self.preview.configure(image="", text="Foto auswählen", padding=(0, 20))
        self.name_var.set("Foto auswählen")
        self.exposure_var.set("–     –     –     –")
        self.selection_var.set("Kein Foto ausgewählt")
        self.extra_var.set("–")
        self.reveal_button.configure(state="disabled")
        for variable in self.detail_vars.values():
            variable.set("–")

    def _selected(self, event=None) -> None:
        selection = self.tree.selection()
        if not selection or selection[0] not in self.rows:
            return
        self._clear_detail()
        generation, root = self._detail_generation, self.target()
        photo_id = int(selection[0])
        self.name_var.set(self.rows[selection[0]]["name"])
        self.preview.configure(text="Vorschau wird geladen …")

        def work():
            try:
                detail = load_metadata_detail(root, photo_id)
                self.dispatcher.post(self._detail_loaded, generation, root, detail)
            except Exception as exc:
                self.dispatcher.post(self._detail_error, generation, str(exc))

        self._detail_future = self._detail_pool.submit(work)

    def _detail_error(self, generation, message) -> None:
        if not self._closed and generation == self._detail_generation:
            self.preview.configure(text=f"Details nicht verfügbar: {message}")

    def _detail_loaded(self, generation, root, detail) -> None:
        if (
            self._closed
            or generation != self._detail_generation
            or root != self.target()
        ):
            return
        if not detail:
            self.preview.configure(
                text="Foto nicht mehr vorhanden. Bitte aktualisieren."
            )
            return
        self.selected_detail = detail
        self.name_var.set(detail["name"])
        if detail["image"] is not None:
            self._render_preview()
        else:
            self.preview.configure(
                image="", text="Für diese Datei ist keine Vorschau verfügbar."
            )

        def number(key, prefix="", suffix=""):
            value = detail.get(key)
            return (
                f"{prefix}{value:g}{suffix}" if isinstance(value, (int, float)) else "–"
            )

        self.exposure_var.set(
            f"{exposure_text(detail.get('exposure'))}     {number('aperture', 'f/')}     {number('iso', 'ISO ')}     {number('focal', suffix=' mm')}"
        )
        self.detail_vars["dt_original"].set(display_date(detail.get("dt_original")))
        self.detail_vars["model"].set(detail.get("model") or "–")
        self.detail_vars["lens"].set(detail.get("lens") or "–")
        self.detail_vars["resolution"].set(
            f"{detail['width']} × {detail['height']} px"
            if detail.get("width") and detail.get("height")
            else "–"
        )
        size = detail["size"]
        self.detail_vars["size"].set(
            (
                f"{size / (1024 * 1024):.1f} MB"
                if size >= 1024 * 1024
                else f"{size / 1024:.1f} KB"
                if size >= 1024
                else f"{size} B"
            ).replace(".", ",")
        )
        rating = detail.get("rating")
        self.detail_vars["rating"].set(
            "★" * max(0, min(5, int(rating))) + "☆" * (5 - max(0, min(5, int(rating))))
            if isinstance(rating, (int, float)) and rating >= 0
            else "–"
        )
        variants = " + ".join(
            dict.fromkeys(item["ext"].upper() for item in detail["variants"])
        )
        self.selection_var.set(f"1 Foto ausgewählt · {variants}")
        extras = [
            ("Hersteller", detail.get("make")),
            ("Seriennummer", detail.get("serial")),
            ("Auslösungen", detail.get("shutter_count")),
            ("Subsekunden", detail.get("subsec")),
            ("Zeitzone", detail.get("offset_time")),
            ("GPS Breite", detail.get("lat")),
            ("GPS Länge", detail.get("lon")),
            ("Höhe (m)", detail.get("alt")),
            ("Farbmarkierung", detail.get("label")),
            (
                "Metadatenstatus",
                {
                    "done": "Vollständig",
                    "pending": "Ausstehend",
                    "stale": "Erneut lesen",
                    "error": "Fehler",
                }.get(detail["meta_state"], detail["meta_state"]),
            ),
            ("Pfad", detail["relpath"]),
        ]
        if detail.get("meta_error"):
            extras.append(("Lesefehler", detail["meta_error"]))
        self.extra_var.set(
            "\n".join(
                f"{label}: {value if value is not None else '–'}"
                for label, value in extras
            )
        )
        self.reveal_button.configure(state="normal" if detail["path"] else "disabled")

    def _toggle_extra(self) -> None:
        visible = bool(self.extra.winfo_manager())
        self.extra.pack_forget() if visible else self.extra.pack(fill="x")
        self.extra_button.configure(
            text="▸ Weitere Metadaten" if visible else "▾ Weitere Metadaten"
        )

    def _preview_resized(self, event=None) -> None:
        if self._resize_timer:
            self.after_cancel(self._resize_timer)
        if not self._closed:
            self._resize_timer = self.after(60, self._render_preview)

    def _render_preview(self) -> None:
        self._resize_timer = None
        if (
            self._closed
            or not self.selected_detail
            or self.selected_detail["image"] is None
        ):
            return
        image = self.selected_detail["image"].copy()
        image.thumbnail(
            (
                max(1, min(480, self.preview.winfo_width())),
                max(140, min(245, self.inspector.winfo_height() - 440)),
            )
        )
        self.preview_image = ImageTk.PhotoImage(image, master=self)
        self.preview.configure(image=self.preview_image, text="", padding=0)

    def reveal(self) -> None:
        if not self.selected_detail or not self.selected_detail["path"]:
            return
        path = self.selected_detail["path"]
        try:
            if os.name == "nt":
                subprocess.Popen(["explorer.exe", "/select,", str(path)])
            elif sys.platform == "darwin":
                subprocess.Popen(["open", "-R", str(path)])
            else:
                subprocess.Popen(["xdg-open", str(path.parent)])
        except OSError as exc:
            self.footer.set(f"Ordner konnte nicht geöffnet werden: {exc}")

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self._resize_timer:
            self.after_cancel(self._resize_timer)
        if self._debounce:
            self.after_cancel(self._debounce)
        self._query_pool.shutdown(wait=False, cancel_futures=True)
        self._detail_pool.shutdown(wait=False, cancel_futures=True)
        self.dispatcher.close()

    def destroy(self) -> None:
        self.close()
        super().destroy()
