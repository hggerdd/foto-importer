"""Folder selection widget."""

from __future__ import annotations

from tkinter import ttk, filedialog
from pathlib import Path
from collections.abc import Callable


class FolderSelector(ttk.Frame):
    """Widget for selecting source and target folders."""

    def __init__(
        self,
        parent: ttk.Frame,
        on_source_selected: Callable[[Path], None] | None = None,
        on_target_selected: Callable[[Path], None] | None = None,
        initial_source: str = "",
        initial_target: str = "",
        target_parent: ttk.Frame | None = None,
    ) -> None:
        """Initialize folder selector.

        Args:
            parent: Parent frame
            on_source_selected: Callback when source folder is selected
            on_target_selected: Callback when target folder is selected
            initial_source: Initial source folder path
            initial_target: Initial target folder path
        """
        super().__init__(parent)
        self.target_parent = target_parent or self

        self.on_source_selected: Callable[[Path], None] | None = on_source_selected
        self.on_target_selected: Callable[[Path], None] | None = on_target_selected
        self.last_source: str = initial_source
        self.last_target: str = initial_target

        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        ttk.Label(self, text="QUELLE", style="Small.TLabel").grid(
            row=0, column=0, padx=(0, 12)
        )
        self.source_label = ttk.Label(
            self, text=self.last_source or "Nicht ausgewählt", width=28
        )
        self.source_label.grid(row=0, column=1, sticky="ew", padx=(0, 12))
        ttk.Button(self, text="Ändern", command=self._select_source_folder).grid(
            row=0, column=2
        )
        target = self.target_parent
        ttk.Label(target, text="ZIELORDNER", style="Small.TLabel").grid(
            row=1, column=0, columnspan=2, sticky="w"
        )
        self.target_label = ttk.Label(
            target, text=self.last_target or "Nicht ausgewählt", wraplength=210
        )
        self.target_label.grid(row=2, column=0, sticky="w", pady=(8, 0), padx=(0, 8))
        ttk.Button(target, text="Ändern", command=self._select_target_folder).grid(
            row=2, column=1, sticky="e", pady=(8, 0)
        )
        self.columnconfigure(1, weight=1)

    def _select_source_folder(self) -> None:
        """Handle source folder selection."""
        initial_dir = self.last_source if self.last_source else None
        folder = filedialog.askdirectory(
            title="Quellordner auswählen", initialdir=initial_dir
        )
        if folder:
            self.last_source = folder
            folder_path = Path(folder)
            self.source_label.config(text=str(folder_path), foreground="black")
            if self.on_source_selected:
                self.on_source_selected(folder_path)

    def _select_target_folder(self) -> None:
        """Handle target folder selection."""
        initial_dir = self.last_target if self.last_target else None
        folder = filedialog.askdirectory(
            title="Zielordner auswählen", initialdir=initial_dir
        )
        if folder:
            self.last_target = folder
            folder_path = Path(folder)
            self.target_label.config(text=str(folder_path), foreground="black")
            if self.on_target_selected:
                self.on_target_selected(folder_path)

    def get_source_folder(self) -> Path | None:
        """Get the selected source folder.

        Returns:
            Source folder path or None
        """
        text = self.source_label.cget("text")
        return Path(text) if text != "Nicht ausgewählt" else None

    def get_target_folder(self) -> Path | None:
        """Get the selected target folder.

        Returns:
            Target folder path or None
        """
        text = self.target_label.cget("text")
        return Path(text) if text != "Nicht ausgewählt" else None
