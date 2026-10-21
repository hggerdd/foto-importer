"""UI layout builder for the main window."""
from __future__ import annotations

import tkinter as tk
import webbrowser
from pathlib import Path
from tkinter import filedialog, ttk
from typing import Callable

from config.settings import DNG_CONVERTER_PATH, DNG_DOWNLOAD_URL, Settings
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
        on_date_source_changed: Callable[[], None],
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
        self._build_settings_section(
            settings,
            on_preview_count_changed,
            on_date_source_changed,
        )
        self._build_date_list(on_date_selection_changed)
        self._build_preview_section()
        self._build_action_section(on_custom_name_changed, on_execute_copy)
        self._build_status_bar()
        self._build_progress_section()

        self.columnconfigure(0, weight=1)
        self.rowconfigure(3, weight=1)
        self.rowconfigure(4, weight=2)

    def _build_scan_progress(self) -> None:
        self.scan_progress = ScanProgressWidget(self)
        self.scan_progress.grid(row=1, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

    def _build_settings_section(
        self,
        settings: Settings,
        on_preview_count_changed: Callable[[], None],
        on_date_source_changed: Callable[[], None],
    ) -> None:
        settings_frame = ttk.LabelFrame(self, text="Settings", padding="10")
        settings_frame.grid(row=2, column=0, sticky=(tk.W, tk.E), pady=(0, 10))
        settings_frame.columnconfigure(1, weight=1)

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

        ttk.Label(settings_frame, text="Date Source:").grid(row=1, column=0, sticky=tk.W, pady=(10, 0))
        self.date_source_var = tk.StringVar(value=settings.date_source)
        filesystem_radio = ttk.Radiobutton(
            settings_frame,
            text="Filesystem timestamps",
            value='filesystem',
            variable=self.date_source_var,
            command=on_date_source_changed,
        )
        filesystem_radio.grid(row=1, column=1, sticky=tk.W, pady=(10, 0))

        metadata_radio = ttk.Radiobutton(
            settings_frame,
            text="Photo metadata",
            value='metadata',
            variable=self.date_source_var,
            command=on_date_source_changed,
        )
        metadata_radio.grid(row=2, column=1, sticky=tk.W)

        self.convert_to_dng_var = tk.BooleanVar(value=settings.get('convert_to_dng', False))
        ttk.Checkbutton(
            settings_frame, text="Convert NEF to DNG after copying",
            variable=self.convert_to_dng_var,
            command=lambda: settings.set('convert_to_dng', self.convert_to_dng_var.get()),
        ).grid(row=3, column=0, columnspan=2, sticky=tk.W, pady=(10, 0))
        ttk.Label(settings_frame, text="DNG Converter:").grid(row=4, column=0, sticky=tk.W)
        self.dng_converter_var = tk.StringVar(
            value=settings.get('dng_converter_path', DNG_CONVERTER_PATH),
        )
        converter_entry = ttk.Entry(settings_frame, textvariable=self.dng_converter_var)
        converter_entry.grid(row=4, column=1, sticky=(tk.W, tk.E), padx=(10, 0))
        converter_entry.bind(
            '<FocusOut>', lambda _event: settings.set(
                'dng_converter_path', self.dng_converter_var.get(),
            ),
        )

        def choose_converter() -> None:
            path = filedialog.askopenfilename(
                title="Select Adobe DNG Converter", filetypes=[("Executable", "*.exe")],
            )
            if path:
                self.dng_converter_var.set(path)
                settings.set('dng_converter_path', path)

        ttk.Button(settings_frame, text="Browse...", command=choose_converter).grid(
            row=4, column=2, padx=(10, 0),
        )
        self.dng_version_label = ttk.Label(
            settings_frame, text="DNG Converter: checking version...",
        )
        self.dng_version_label.grid(row=5, column=0, columnspan=2, sticky=tk.W, pady=(5, 0))
        self.dng_download_button = ttk.Button(
            settings_frame, text="Download update", state='disabled',
            command=lambda: webbrowser.open(DNG_DOWNLOAD_URL),
        )
        self.dng_download_button.grid(row=5, column=2, padx=(10, 0), pady=(5, 0))

    def _build_date_list(self, on_selection_changed: Callable[[set[str]], None]) -> None:
        self.date_list = DateListWidget(
            self,
            on_selection_changed=on_selection_changed,
        )
        self.date_list.grid(row=3, column=0, sticky=(tk.W, tk.E, tk.N, tk.S), pady=(0, 10))

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
        progress_container = ttk.LabelFrame(self, text="Copy and DNG Jobs", padding="10")
        progress_container.grid(row=7, column=0, sticky=(tk.W, tk.E), pady=(10, 0))
        self.progress_manager = ProgressManager(progress_container)
