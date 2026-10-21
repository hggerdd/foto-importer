"""Image preview widget for displaying thumbnails."""

from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from PIL import Image, ImageTk
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, Future
from threading import Event

from core.thumbnails import ThumbnailLoader
from core.task_scheduler import TaskScheduler
from ui.dispatcher import UiDispatcher


class PreviewWidget(ttk.Frame):
    """Widget to display image thumbnails in a scrollable area."""

    def __init__(
        self,
        parent: ttk.Frame,
        thumbnail_size: tuple[int, int] = (210, 140),
        scheduler: TaskScheduler | None = None,
    ) -> None:
        super().__init__(parent)
        self._scheduler = scheduler
        self.thumbnail_size: tuple[int, int] = thumbnail_size
        self.image_references: list[
            ImageTk.PhotoImage
        ] = []  # Keep references to prevent garbage collection
        self._loader = ThumbnailLoader()
        self._executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="Preview")
        self._dispatcher = UiDispatcher(self)
        self._generation = 0
        self._cancel_event = Event()
        self._future: Future | None = None
        self._columns = 3
        self._tiles: list[tuple[int, ttk.Frame]] = []
        self._resize_id = None
        self._paths: list[Path] = []
        self._empty_message = "Aufnahmetag auswählen\nDie Bildvorschau erscheint hier."

        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        # Create canvas with scrollbar
        self.canvas = tk.Canvas(self, bg="#ffffff", highlightthickness=0)
        self.scrollbar = ttk.Scrollbar(
            self, orient="vertical", command=self.canvas.yview
        )
        self.scrollable_frame = ttk.Frame(self.canvas)

        self.scrollable_frame.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all")),
        )

        self._window = self.canvas.create_window(
            (0, 0), window=self.scrollable_frame, anchor="nw"
        )
        self.canvas.configure(yscrollcommand=self.scrollbar.set)
        self.canvas.bind("<Configure>", self._schedule_reflow)

        # Pack components
        self.canvas.pack(side="left", fill="both", expand=True)
        self.scrollbar.pack(side="right", fill="y")

        # Bind mouse wheel
        # Bind only this gallery, never steal the wheel from days or settings.
        self.canvas.bind("<MouseWheel>", self._on_mousewheel)
        self.scrollable_frame.bind("<MouseWheel>", self._on_mousewheel)

    def _schedule_reflow(self, event=None) -> None:
        if self._resize_id:
            self.after_cancel(self._resize_id)
        self._resize_id = self.after(80, self._reflow)

    def _reflow(self) -> None:
        self._resize_id = None
        width = max(1, self.canvas.winfo_width())
        self.canvas.itemconfigure(self._window, width=width)
        columns = max(1, width // (self.thumbnail_size[0] + 16))
        self._columns = columns
        for index, frame in self._tiles:
            frame.grid(
                row=index // columns, column=index % columns, padx=7, pady=7, sticky="n"
            )
        self.canvas.configure(scrollregion=self.canvas.bbox("all"))

    def set_thumbnail_size(self, width: int) -> None:
        """Decode resized samples in the preview worker after a zoom change."""
        self.thumbnail_size = (width, width * 2 // 3)
        self.load_previews(self._paths, empty_message=self._empty_message)

    def _on_mousewheel(self, event: tk.Event) -> None:
        """Handle mouse wheel scrolling."""
        self.canvas.yview_scroll(int(-1 * (event.delta / 120)), "units")

    def clear_previews(self) -> None:
        """Clear all preview images."""
        self._generation += 1
        self._cancel_event.set()
        if self._future:
            self._future.cancel()
        for widget in self.scrollable_frame.winfo_children():
            widget.destroy()
        self.image_references.clear()
        self._tiles.clear()
        self._paths = []
        self.canvas.yview_moveto(0)

    def load_previews(
        self,
        image_paths: list[Path],
        columns: int = 3,
        empty_message: str = "Aufnahmetag auswählen\nDie Bildvorschau erscheint hier.",
    ) -> None:
        """
        Load and display image thumbnails.

        Args:
            image_paths: List of image file paths
            columns: Number of columns in the grid
        """
        self.clear_previews()
        self._paths = image_paths
        self._empty_message = empty_message
        self._columns = columns

        if not image_paths:
            label = ttk.Label(
                self.scrollable_frame,
                text=empty_message,
                style="Muted.TLabel",
                justify="center",
            )
            label.grid(row=0, column=0, padx=20, pady=20)
            return

        generation = self._generation
        self._cancel_event = Event()
        cancel_event = self._cancel_event
        size = self.thumbnail_size

        def load() -> None:
            for idx, path in enumerate(image_paths):
                if cancel_event.is_set():
                    return
                try:
                    thumbnail = self._loader.load(path, size)
                except Exception:  # A corrupt image must not stop other previews.
                    thumbnail = None
                if cancel_event.is_set():
                    return
                self._dispatcher.post(
                    self._display_thumbnail,
                    generation,
                    idx,
                    path,
                    thumbnail,
                    max(1, columns),
                    key=("preview", idx),
                )

        lease = self._scheduler.foreground() if self._scheduler else None
        try:
            self._future = self._executor.submit(load)
        except BaseException:
            if lease:
                lease.close()
            raise
        if lease:
            self._future.add_done_callback(lambda _: lease.close())

    def _display_thumbnail(
        self,
        generation: int,
        index: int,
        path: Path,
        thumbnail: Image.Image | None,
        columns: int,
    ) -> None:
        """Create Tk images exclusively on the UI thread."""
        if generation != self._generation:
            return
        columns = max(1, self.canvas.winfo_width() // (self.thumbnail_size[0] + 16))
        frame = ttk.Frame(self.scrollable_frame)
        self._tiles.append((index, frame))
        frame.grid(
            row=index // columns, column=index % columns, padx=7, pady=7, sticky="n"
        )
        if thumbnail is not None:
            photo = ImageTk.PhotoImage(thumbnail, master=self)
            self.image_references.append(photo)
            ttk.Label(frame, image=photo).pack(padx=2, pady=2)
        ttk.Label(
            frame,
            text=path.name if thumbnail is not None else f"Keine Vorschau\n{path.name}",
            foreground="" if thumbnail is not None else "red",
            font=("Segoe UI", 9),
            wraplength=self.thumbnail_size[0],
        ).pack(padx=2, pady=2)
        for widget in (frame, *frame.winfo_children()):
            widget.bind("<MouseWheel>", self._on_mousewheel)

    def destroy(self) -> None:
        """Clean up resources."""
        self._cancel_event.set()
        if self._resize_id:
            self.after_cancel(self._resize_id)
        self._executor.shutdown(wait=False, cancel_futures=True)
        self._dispatcher.close()
        super().destroy()
