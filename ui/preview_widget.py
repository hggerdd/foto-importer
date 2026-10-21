"""Virtual gallery: reserve the full grid, render only the current viewport."""

from __future__ import annotations

from dataclasses import dataclass
from math import ceil
from pathlib import Path
import tkinter as tk
from tkinter import ttk

from PIL import Image, ImageTk

from config.settings import PREVIEW_PREFETCH_ROWS
from core.preview_worker import PreviewWorker
from core.task_scheduler import TaskScheduler
from core.thumbnails import ThumbnailLoader
from ui.dispatcher import UiDispatcher


@dataclass
class PreviewTile:
    placeholder: int
    image: int
    caption: int


class PreviewWidget(ttk.Frame):
    """Bound canvas items and Tk images to visible cells, irrespective of file count."""

    def __init__(
        self,
        parent: tk.Misc,
        thumbnail_size: tuple[int, int] = (210, 140),
        scheduler: TaskScheduler | None = None,
    ) -> None:
        super().__init__(parent)
        self.thumbnail_size = thumbnail_size
        self.image_references: dict[int, ImageTk.PhotoImage] = {}
        self._loader = ThumbnailLoader()
        self._worker = PreviewWorker(self._loader, scheduler)
        self._dispatcher = UiDispatcher(self)
        self._generation = 0
        self._columns = 1
        self._tiles: dict[int, PreviewTile] = {}
        self._paths: list[Path] = []
        self._ready_images: dict[int, Image.Image | None] = {}
        self._visible_indices: set[int] = set()
        self._wanted_indices: set[int] = set()
        self._refresh_id = None
        self._request_signature = None
        self._layout_signature = None
        self._last_yview = None
        self._top = 0.0
        self._empty_message = "Aufnahmetag auswählen\nDie Bildvorschau erscheint hier."
        self.canvas = tk.Canvas(self, bg="white", highlightthickness=0, takefocus=True)
        self.scrollbar = ttk.Scrollbar(
            self, orient="vertical", command=self.canvas.yview
        )
        self.canvas.configure(yscrollcommand=self._on_yview)
        self.scrollbar.pack(side="right", fill="y")
        self.canvas.pack(side="left", fill="both", expand=True)
        self.canvas.bind("<Configure>", self._schedule_refresh)
        self.canvas.bind("<MouseWheel>", self._on_mousewheel)
        self.canvas.bind("<Button-1>", lambda event: self.canvas.focus_set())
        for key, amount, units in (
            ("Prior", -1, "pages"),
            ("Next", 1, "pages"),
            ("Up", -1, "units"),
            ("Down", 1, "units"),
        ):
            self.canvas.bind(
                f"<{key}>", lambda event, n=amount, u=units: self._scroll(n, u)
            )
        self.canvas.bind("<Home>", lambda event: self.canvas.yview_moveto(0))
        self.canvas.bind("<End>", lambda event: self.canvas.yview_moveto(1))

    @property
    def viewport_ready(self) -> bool:
        """Whether visible cells have a decoded thumbnail or a recorded error."""
        return self._visible_indices.issubset(self._ready_images)

    def _on_yview(self, first: str, last: str) -> None:
        self.scrollbar.set(first, last)
        if self._last_yview != (first, last):
            self._last_yview = (first, last)
            self._schedule_refresh()

    def _schedule_refresh(self, event=None) -> None:
        if self._refresh_id is None:
            self._refresh_id = self.after(16, self._refresh_viewport)

    def _scroll(self, amount: int, units: str) -> str:
        self.canvas.yview_scroll(amount, units)
        return "break"

    def _on_mousewheel(self, event: tk.Event) -> str:
        return self._scroll(-int(event.delta / 120), "units")

    def _reset_tiles(self) -> None:
        self.canvas.delete("all")
        self._tiles.clear()
        self.image_references.clear()

    def clear_previews(self) -> None:
        """Cancel decoding and release viewport images without waiting for I/O."""
        self._generation += 1
        self._worker.cancel()
        if self._refresh_id:
            self.after_cancel(self._refresh_id)
            self._refresh_id = None
        self._reset_tiles()
        self._ready_images.clear()
        self._visible_indices.clear()
        self._wanted_indices.clear()
        self._paths = []
        self._request_signature = self._layout_signature = None
        self.canvas.configure(scrollregion=(0, 0, 1, 1))
        self.canvas.yview_moveto(0)
        self._top = 0.0

    def load_previews(
        self,
        image_paths: list[Path],
        *,
        empty_message: str = "Aufnahmetag auswählen\nDie Bildvorschau erscheint hier.",
    ) -> None:
        """Index every image immediately, with no decoding or per-file Tk objects."""
        self.clear_previews()
        self._paths = image_paths
        self._empty_message = empty_message
        self._refresh_viewport()

    def set_thumbnail_size(self, width: int) -> None:
        """Invalidate old sizes, preserving the first visible image when zooming."""
        if width == self.thumbnail_size[0]:
            return
        anchor = min(self._visible_indices, default=0)
        paths, message = self._paths, self._empty_message
        self.clear_previews()
        self.thumbnail_size = (width, width * 2 // 3)
        self._paths, self._empty_message = paths, message
        self._refresh_viewport(anchor)

    def _refresh_viewport(self, anchor: int | None = None) -> None:
        if self._refresh_id:
            self.after_cancel(self._refresh_id)
            self._refresh_id = None
        width, height = (
            max(1, self.canvas.winfo_width()),
            max(1, self.canvas.winfo_height()),
        )
        if not self._paths:
            self.canvas.delete("empty")
            self.canvas.create_text(
                width / 2,
                40,
                text=self._empty_message,
                width=max(180, width - 30),
                anchor="n",
                font=("Segoe UI", 11),
                fill="#68717e",
                tags="empty",
            )
            return
        cell_width, cell_height = (
            self.thumbnail_size[0] + 16,
            self.thumbnail_size[1] + 44,
        )
        columns = max(1, width // cell_width)
        rows = ceil(len(self._paths) / columns)
        total_height = rows * cell_height
        layout = (columns, total_height, self.thumbnail_size)
        if layout != self._layout_signature:
            if anchor is None and self._visible_indices:
                anchor = min(self._visible_indices)
            self._columns = columns
            self._layout_signature = layout
            self._reset_tiles()
            self.canvas.configure(
                scrollregion=(0, 0, width, total_height),
                yscrollincrement=max(1, cell_height // 3),
            )
            if anchor is not None:
                self.canvas.yview_moveto(
                    (anchor // columns * cell_height) / total_height
                )
        top = max(0.0, self.canvas.canvasy(0))
        first_row = min(rows - 1, int(top // cell_height))
        last_row = min(rows, max(first_row + 1, ceil((top + height) / cell_height)))
        first, last = first_row * columns, min(len(self._paths), last_row * columns)
        visible = set(range(first, last))
        before = list(range(max(0, first - PREVIEW_PREFETCH_ROWS * columns), first))
        after = list(
            range(last, min(len(self._paths), last + PREVIEW_PREFETCH_ROWS * columns))
        )
        order = list(range(first, last)) + (
            after + before if top >= self._top else before[::-1] + after
        )
        self._top = top
        self._visible_indices, self._wanted_indices = visible, set(order)
        self._ready_images = {
            i: image
            for i, image in self._ready_images.items()
            if i in self._wanted_indices
        }
        for index in set(self._tiles) - visible:
            tile = self._tiles.pop(index)
            for item in (tile.placeholder, tile.image, tile.caption):
                self.canvas.delete(item)
            self.image_references.pop(index, None)
        for index in sorted(visible - self._tiles.keys()):
            self._create_tile(index, cell_width, cell_height)
            if index in self._ready_images:
                self._display_thumbnail(index, self._ready_images[index])
        missing = tuple(i for i in order if i not in self._ready_images)
        # Results arrive in UI time slices. Do not restart an unchanged viewport
        # merely because some results from its current batch are still queued.
        signature = (self._generation, tuple(order), self.thumbnail_size)
        if missing and signature != self._request_signature:
            self._request_signature = signature
            self._worker.request(
                self._generation,
                [(i, self._paths[i]) for i in missing],
                self.thumbnail_size,
                lambda generation, index, thumbnail: self._dispatcher.post(
                    self._accept_thumbnail,
                    generation,
                    index,
                    thumbnail,
                    key=("preview", index),
                ),
            )
        elif not missing:
            self._request_signature = None
            self._worker.cancel()

    def _create_tile(self, index: int, cell_width: int, cell_height: int) -> None:
        x, y = (
            (index % self._columns) * cell_width + 7,
            (index // self._columns) * cell_height + 7,
        )
        width, height = self.thumbnail_size
        self._tiles[index] = PreviewTile(
            self.canvas.create_rectangle(
                x, y, x + width, y + height, fill="white", outline="#eef0f3"
            ),
            self.canvas.create_image(x + width / 2, y + height / 2),
            self.canvas.create_text(
                x + width / 2,
                y + height + 7,
                text=self._paths[index].name,
                width=width,
                anchor="n",
                font=("Segoe UI", 9),
                fill="#68717e",
            ),
        )

    def _accept_thumbnail(
        self, generation: int, index: int, thumbnail: Image.Image | None
    ) -> None:
        if generation != self._generation or index not in self._wanted_indices:
            return
        self._ready_images[index] = thumbnail
        if index in self._tiles:
            self._display_thumbnail(index, thumbnail)

    def _display_thumbnail(self, index: int, thumbnail: Image.Image | None) -> None:
        """Create and retain Tk images only for cells currently on screen."""
        tile = self._tiles[index]
        if thumbnail is None:
            self.canvas.itemconfigure(
                tile.caption,
                text=f"Keine Vorschau\n{self._paths[index].name}",
                fill="#b42318",
            )
        else:
            photo = ImageTk.PhotoImage(thumbnail, master=self)
            self.image_references[index] = photo
            self.canvas.itemconfigure(tile.image, image=photo)

    def destroy(self) -> None:
        self._worker.close()
        if self._refresh_id:
            self.after_cancel(self._refresh_id)
        self._dispatcher.close()
        super().destroy()
