"""Main application window for camera organizer."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk, messagebox
from pathlib import Path

from config.settings import Settings
from core.file_manager import FileManager
from core.copy_worker import CopyWorker, JobState
from ui.folder_selector import FolderSelector
from ui.date_list_widget import DateListWidget
from ui.preview_widget import PreviewWidget
from ui.progress_manager import ProgressManager


class MainWindow:
    """Main application window."""

    def __init__(self, root: tk.Tk) -> None:
        """Initialize main window.

        Args:
            root: Root Tk instance
        """
        self.root: tk.Tk = root
        self.root.title("Camera File Organizer")

        # Initialize settings
        self.settings: Settings = Settings()
        self.root.geometry(self.settings.window_geometry)

        # Initialize managers
        self.file_manager: FileManager = FileManager()
        self.copy_worker: CopyWorker = CopyWorker()

        # State
        self.source_folder: Path | None = None
        self.target_folder: Path | None = None

        # Setup UI
        self._setup_ui()

        # Bind window close event to save settings
        self.root.protocol("WM_DELETE_WINDOW", self._on_closing)

    def _setup_ui(self) -> None:
        """Setup the main UI."""
        # Main container
        main_container = ttk.Frame(self.root, padding="10")
        main_container.grid(row=0, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)

        # Folder selection
        self.folder_selector = FolderSelector(
            main_container,
            on_source_selected=self._on_source_selected,
            on_target_selected=self._on_target_selected,
            initial_source=self.settings.last_source_folder,
            initial_target=self.settings.last_target_folder
        )
        self.folder_selector.grid(row=0, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        # Settings section
        self._setup_settings(main_container)

        # Date list
        self.date_list = DateListWidget(
            main_container,
            on_selection_changed=self._on_date_selection_changed
        )
        self.date_list.grid(row=2, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        # Preview section
        preview_frame = ttk.LabelFrame(main_container, text="Image Preview", padding="10")
        preview_frame.grid(row=3, column=0, sticky=(tk.W, tk.E, tk.N, tk.S), pady=(0, 10))
        self.preview_widget = PreviewWidget(preview_frame)
        self.preview_widget.pack(fill='both', expand=True)
        preview_frame.columnconfigure(0, weight=1)
        preview_frame.rowconfigure(0, weight=1)

        # Action section
        self._setup_action_section(main_container)

        # Status bar
        self._setup_status_bar(main_container)

        # Progress bars
        progress_container = ttk.LabelFrame(main_container, text="Active Copy Jobs", padding="10")
        progress_container.grid(row=6, column=0, sticky=(tk.W, tk.E), pady=(10, 0))
        self.progress_manager = ProgressManager(progress_container)

        # Configure grid weights
        main_container.columnconfigure(0, weight=1)
        main_container.rowconfigure(3, weight=1)

    def _setup_settings(self, parent: ttk.Frame) -> None:
        """Setup settings section."""
        settings_frame = ttk.LabelFrame(parent, text="Settings", padding="10")
        settings_frame.grid(row=1, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        ttk.Label(settings_frame, text="Preview Count:").grid(row=0, column=0, sticky=tk.W)
        self.preview_count_var = tk.IntVar(value=self.settings.preview_count)
        preview_spinbox = ttk.Spinbox(
            settings_frame,
            from_=1,
            to=50,
            textvariable=self.preview_count_var,
            width=10,
            command=self._on_preview_count_changed
        )
        preview_spinbox.grid(row=0, column=1, sticky=tk.W, padx=(10, 0))

    def _setup_action_section(self, parent: ttk.Frame) -> None:
        """Setup action buttons section."""
        action_frame = ttk.Frame(parent, padding="10")
        action_frame.grid(row=4, column=0, sticky=(tk.W, tk.E), pady=(0, 10))

        ttk.Label(action_frame, text="Custom Name:").grid(row=0, column=0, sticky=tk.W)
        self.custom_name_var = tk.StringVar()
        self.custom_name_var.trace_add('write', lambda *args: self._update_execute_button_state())
        self.custom_name_entry = ttk.Entry(action_frame, textvariable=self.custom_name_var, width=40)
        self.custom_name_entry.grid(row=0, column=1, sticky=tk.W, padx=(10, 20))
        self.custom_name_entry.bind('<Return>', lambda e: self._execute_copy())

        self.execute_button = ttk.Button(
            action_frame,
            text="Execute Copy",
            command=self._execute_copy,
            state='disabled'
        )
        self.execute_button.grid(row=0, column=2)

    def _setup_status_bar(self, parent: ttk.Frame) -> None:
        """Setup status bar."""
        status_frame = ttk.Frame(parent)
        status_frame.grid(row=5, column=0, sticky=(tk.W, tk.E))

        self.status_label = ttk.Label(status_frame, text="Ready", relief=tk.SUNKEN, anchor=tk.W)
        self.status_label.pack(fill=tk.X)

    def _on_source_selected(self, folder_path: Path) -> None:
        """Handle source folder selection."""
        self.source_folder = folder_path
        self.settings.last_source_folder = str(folder_path)
        self._scan_source_folder()

    def _on_target_selected(self, folder_path: Path) -> None:
        """Handle target folder selection."""
        self.target_folder = folder_path
        self.settings.last_target_folder = str(folder_path)
        self._update_execute_button_state()

    def _on_preview_count_changed(self) -> None:
        """Handle preview count change."""
        self.settings.preview_count = self.preview_count_var.get()

    def _scan_source_folder(self) -> None:
        """Scan source folder for files."""
        self.status_label.config(text="Scanning files...")
        self.root.update()

        try:
            files_by_date = self.file_manager.set_source_folder(str(self.source_folder))
            self._populate_date_list()
            self.status_label.config(
                text=f"Found {len(files_by_date)} date groups with files"
            )
        except Exception as e:
            messagebox.showerror("Error", f"Error scanning folder: {e}")
            self.status_label.config(text="Error scanning folder")

    def _populate_date_list(self) -> None:
        """Populate the date listbox."""
        date_groups = self.file_manager.get_date_groups()
        date_counts = {date: self.file_manager.get_file_count(date) for date in date_groups}
        self.date_list.populate(date_counts)

    def _on_date_selection_changed(self, selected_dates: set[str]) -> None:
        """Handle date selection change."""
        if not selected_dates:
            return

        # Show preview for first selected date
        first_date = next(iter(selected_dates))
        self._show_preview(first_date)
        self._update_execute_button_state()

    def _show_preview(self, date_str: str) -> None:
        """Show preview images for selected date."""
        self.status_label.config(text=f"Loading preview for {date_str}...")
        self.root.update()

        try:
            preview_count = self.preview_count_var.get()
            image_paths = self.file_manager.get_image_files_for_preview(date_str, preview_count)
            self.preview_widget.load_previews(image_paths)
            self.status_label.config(
                text=f"Showing {len(image_paths)} preview images for {date_str}"
            )
        except Exception as e:
            messagebox.showerror("Error", f"Error loading preview: {e}")
            self.status_label.config(text="Error loading preview")

    def _update_execute_button_state(self) -> None:
        """Update execute button state based on selections."""
        selected_dates = self.date_list.get_selected_dates()
        has_selection = bool(selected_dates and self.target_folder and self.custom_name_var.get().strip())

        if has_selection:
            self.execute_button.config(state='normal')
        else:
            self.execute_button.config(state='disabled')

    def _execute_copy(self) -> None:
        """Execute the copy operation."""
        selected_dates = self.date_list.get_selected_dates()

        if not selected_dates:
            messagebox.showwarning("Warning", "Please select at least one date group")
            return

        if not self.target_folder:
            messagebox.showwarning("Warning", "Please select target folder")
            return

        custom_name = self.custom_name_var.get().strip()
        if not custom_name:
            messagebox.showwarning("Warning", "Please enter a custom name")
            return

        # Collect all files from selected dates
        all_files: list[Path] = []
        for date_str in selected_dates:
            all_files.extend(self.file_manager.get_files_for_date(date_str))

        if not all_files:
            messagebox.showwarning("Warning", "No files to copy")
            return

        # Check if target folder already exists
        target_path = self.target_folder / custom_name
        if target_path.exists():
            response = messagebox.askyesno(
                "Folder Exists",
                f"The folder '{custom_name}' already exists in the target location.\n\n"
                f"Files will be added to existing subfolders.\n\n"
                f"Continue?"
            )
            if not response:
                return

        # Start copy operation
        self.status_label.config(text=f"Copying {len(all_files)} files...")
        self.execute_button.config(state='disabled')

        # Add progress bar for this job
        self.progress_manager.add_progress_bar(custom_name, len(all_files))

        # Remove date groups from list IMMEDIATELY (before copy starts)
        for date_str in selected_dates:
            self.file_manager.remove_date_group(date_str)

        self._populate_date_list()
        self.preview_widget.clear_previews()

        self.copy_worker.copy_files(
            files=all_files,
            target_folder=self.target_folder,
            custom_name=custom_name,
            on_complete=lambda name: self.root.after(0, self._on_copy_complete, name),
            on_error=lambda name, err: self.root.after(0, self._on_copy_error, name, err),
            on_progress=lambda current, total: self.root.after(
                0, self._on_copy_progress, custom_name, current, total
            ),
            on_status_change=lambda name, state: self.root.after(
                0, self._on_copy_status_change, name, state
            )
        )

        # Clear selection for next operation
        self.custom_name_var.set("")

    def _on_copy_progress(self, job_name: str, current: int, total: int) -> None:
        """Handle copy progress update."""
        self.progress_manager.update_progress(job_name, current, total)

    def _on_copy_complete(self, job_name: str) -> None:
        """Handle copy completion."""
        self.progress_manager.remove_progress_bar(job_name)
        self.status_label.config(text=f"Copy completed: {job_name}")

    def _on_copy_error(self, job_name: str, error_msg: str) -> None:
        """Handle copy error."""
        self.progress_manager.remove_progress_bar(job_name)
        self.status_label.config(text=f"Copy failed: {error_msg}")
        messagebox.showerror("Error", f"Copy failed for {job_name}:\n{error_msg}")
        self._update_execute_button_state()

    def _on_copy_status_change(self, job_name: str, state: JobState) -> None:
        """Handle lifecycle updates for copy jobs."""
        if state is JobState.QUEUED:
            self.status_label.config(text=f"Queued copy job: {job_name}")
        elif state is JobState.RUNNING:
            self.status_label.config(text=f"Copy in progress: {job_name}")
        elif state is JobState.CANCELLED:
            self.progress_manager.remove_progress_bar(job_name)
            self.status_label.config(text=f"Copy cancelled: {job_name}")

    def _on_closing(self) -> None:
        """Handle window closing event."""
        # Save window geometry
        self.settings.window_geometry = self.root.geometry()
        self.root.destroy()
