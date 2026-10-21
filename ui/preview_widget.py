"""Image preview widget for displaying thumbnails."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from PIL import Image, ImageTk
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, Future
from threading import Event

from core.thumbnails import ThumbnailLoader
from ui.dispatcher import UiDispatcher


class PreviewWidget(ttk.Frame):
    """Widget to display image thumbnails in a scrollable area."""

    def __init__(self, parent: ttk.Frame, thumbnail_size: tuple[int, int] = (150, 150)) -> None:
        super().__init__(parent)
        self.thumbnail_size: tuple[int, int] = thumbnail_size
        self.image_references: list[ImageTk.PhotoImage] = []  # Keep references to prevent garbage collection
        self._loader = ThumbnailLoader()
        self._executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="Preview")
        self._dispatcher = UiDispatcher(self)
        self._generation = 0
        self._cancel_event = Event()
        self._future: Future | None = None

        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        # Create canvas with scrollbar
        self.canvas = tk.Canvas(self, bg='#f0f0f0', highlightthickness=0)
        self.scrollbar = ttk.Scrollbar(self, orient='vertical', command=self.canvas.yview)
        self.scrollable_frame = ttk.Frame(self.canvas)

        self.scrollable_frame.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all"))
        )

        self.canvas.create_window((0, 0), window=self.scrollable_frame, anchor="nw")
        self.canvas.configure(yscrollcommand=self.scrollbar.set)

        # Pack components
        self.canvas.pack(side='left', fill='both', expand=True)
        self.scrollbar.pack(side='right', fill='y')

        # Bind mouse wheel
        self.canvas.bind_all("<MouseWheel>", self._on_mousewheel)

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

    def load_previews(self, image_paths: list[Path], columns: int = 5) -> None:
        """
        Load and display image thumbnails.

        Args:
            image_paths: List of image file paths
            columns: Number of columns in the grid
        """
        self.clear_previews()

        if not image_paths:
            label = ttk.Label(
                self.scrollable_frame,
                text="No images to preview",
                font=('Arial', 12)
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
                    self._display_thumbnail, generation, idx, path, thumbnail,
                    max(1, columns), key=("preview", idx),
                )

        self._future = self._executor.submit(load)

    def _display_thumbnail(
        self, generation: int, index: int, path: Path,
        thumbnail: Image.Image | None, columns: int,
    ) -> None:
        """Create Tk images exclusively on the UI thread."""
        if generation != self._generation:
            return
        frame = ttk.Frame(self.scrollable_frame, relief='solid', borderwidth=1)
        frame.grid(row=index // columns, column=index % columns, padx=5, pady=5)
        if thumbnail is not None:
            photo = ImageTk.PhotoImage(thumbnail, master=self)
            self.image_references.append(photo)
            ttk.Label(frame, image=photo).pack(padx=2, pady=2)
        ttk.Label(
            frame,
            text=path.name if thumbnail is not None else f"Error loading\n{path.name}",
            foreground='' if thumbnail is not None else 'red',
            font=('Arial', 8), wraplength=self.thumbnail_size[0],
        ).pack(padx=2, pady=2)

    def destroy(self) -> None:
        """Clean up resources."""
        self._cancel_event.set()
        self._executor.shutdown(wait=False, cancel_futures=True)
        self._dispatcher.close()
        self.canvas.unbind_all("<MouseWheel>")
        super().destroy()
