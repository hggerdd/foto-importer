"""UI layout builder for the main window."""
from __future__ import annotations

import tkinter as tk
from pathlib import Path
from tkinter import ttk
from typing import Callable

from config.settings import Settings
from ui.date_list_widget import DateListWidget
from ui.folder_selector import FolderSelector
from ui.preview_widget import PreviewWidget
from ui.progress_manager import ProgressManager
from ui.scan_progress import ScanProgressWidget


class MainLayout(ttk.Frame):
    """Constructs and exposes the main window widgets."""

    def __init__(
        self,
        root: tk.Tk,
        settings: Settings,
        *,
        on_source_selected: Callable[[Path], None],
        on_target_selected: Callable[[Path], None],
        on_preview_count_changed: Callable[[], None],
        on_date_selection_changed: Callable[[set[str]], None],
        on_custom_name_changed: Callable[[], None],
        on_execute_copy: Callable[[], None],
    ) -> None:
        super().__init__(root, padding="10")
        self.grid(row=0, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        root.columnconfigure(0, weight=1)
        root.rowconfigure(0, weight=1)

        self.folder_selector = FolderSelector(
            self,
            on_source_selected=on_source_selected,
            on_target_selected=on_target_selected,
            initial_source=settings.last_source_folder,
            initial_target=settings.last_target_folder,
        )
        self.folder_selector.grid(row=0, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        self._build_scan_progress()
        self._build_settings_section(settings, on_preview_count_changed)
        self._build_date_list(on_date_selection_changed)
        self._build_preview_section()
        self._build_action_section(on_custom_name_changed, on_execute_copy)
        self._build_status_bar()
        self._build_progress_section()

        self.columnconfigure(0, weight=1)
        self.rowconfigure(4, weight=1)

    def _build_scan_progress(self) -> None:
        self.scan_progress = ScanProgressWidget(self)
        self.scan_progress.grid(row=1, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

    def _build_settings_section(
        self,
        settings: Settings,
        on_preview_count_changed: Callable[[], None],
    ) -> None:
        settings_frame = ttk.LabelFrame(self, text="Settings", padding="10")
        settings_frame.grid(row=2, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        ttk.Label(settings_frame, text="Preview Count:").grid(row=0, column=0, sticky=tk.W)
        self.preview_count_var = tk.IntVar(value=settings.preview_count)
        preview_spinbox = ttk.Spinbox(
            settings_frame,
            from_=1,
            to=50,
            textvariable=self.preview_count_var,
            width=10,
            command=on_preview_count_changed,
        )
        preview_spinbox.grid(row=0, column=1, sticky=tk.W, padx=(10, 0))

    def _build_date_list(self, on_selection_changed: Callable[[set[str]], None]) -> None:
        self.date_list = DateListWidget(
            self,
            on_selection_changed=on_selection_changed,
        )
        self.date_list.grid(row=3, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

    def _build_preview_section(self) -> None:
        preview_frame = ttk.LabelFrame(self, text="Image Preview", padding="10")
        preview_frame.grid(row=4, column=0, sticky=(tk.W, tk.E, tk.N, tk.S), pady=(0, 10))
        self.preview_widget = PreviewWidget(preview_frame)
        self.preview_widget.pack(fill='both', expand=True)
        preview_frame.columnconfigure(0, weight=1)
        preview_frame.rowconfigure(0, weight=1)

    def _build_action_section(
        self,
        on_custom_name_changed: Callable[[], None],
        on_execute_copy: Callable[[], None],
    ) -> None:
        action_frame = ttk.Frame(self, padding="10")
        action_frame.grid(row=5, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        ttk.Label(action_frame, text="Custom Name:").grid(row=0, column=0, sticky=tk.W)
        self.custom_name_var = tk.StringVar()
        self.custom_name_var.trace_add('write', lambda *args: on_custom_name_changed())

        self.custom_name_entry = ttk.Entry(
            action_frame,
            textvariable=self.custom_name_var,
            width=40,
        )
        self.custom_name_entry.grid(row=0, column=1, sticky=tk.W, padx=(10, 20))
        self.custom_name_entry.bind('<Return>', lambda _event: on_execute_copy())

        self.execute_button = ttk.Button(
            action_frame,
            text="Execute Copy",
            command=on_execute_copy,
            state='disabled',
        )
        self.execute_button.grid(row=0, column=2)

    def _build_status_bar(self) -> None:
        status_frame = ttk.Frame(self)
        status_frame.grid(row=6, column=0, sticky=(tk.W, tk.E))

        self.status_label = ttk.Label(status_frame, text="Ready", relief=tk.SUNKEN, anchor=tk.W)
        self.status_label.pack(fill=tk.X)

    def _build_progress_section(self) -> None:
        progress_container = ttk.LabelFrame(self, text="Active Copy Jobs", padding="10")
        progress_container.grid(row=7, column=0, sticky=(tk.W, tk.E), pady=(10, 0))
        self.progress_manager = ProgressManager(progress_container)
