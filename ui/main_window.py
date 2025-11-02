"""Main application window for camera organizer."""
from __future__ import annotations

import tkinter as tk
from pathlib import Path
from tkinter import messagebox

from config.settings import Settings
from core.copy_worker import CopyWorker
from core.file_manager import FileManager
from ui.controllers import CopyJobController, SourceScanController
from ui.layout import MainLayout


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
        self._pending_groups: dict[str, dict[str, list[Path]]] = {}

        # Setup UI layout and widgets
        self.layout = MainLayout(
            root=self.root,
            settings=self.settings,
            on_source_selected=self._on_source_selected,
            on_target_selected=self._on_target_selected,
            on_preview_count_changed=self._on_preview_count_changed,
            on_date_selection_changed=self._on_date_selection_changed,
            on_custom_name_changed=self._update_execute_button_state,
            on_execute_copy=self._execute_copy,
        )

        self.folder_selector = self.layout.folder_selector
        self.date_list = self.layout.date_list
        self.preview_widget = self.layout.preview_widget
        self.preview_count_var = self.layout.preview_count_var
        self.custom_name_var = self.layout.custom_name_var
        self.custom_name_entry = self.layout.custom_name_entry
        self.execute_button = self.layout.execute_button
        self.status_label = self.layout.status_label
        self.scan_progress = self.layout.scan_progress
        self.progress_manager = self.layout.progress_manager

        # Controllers for background tasks
        self.scan_controller = SourceScanController(self.root, self.file_manager)
        self.job_controller = CopyJobController(
            self.root,
            self.copy_worker,
            self.progress_manager,
        )
        self.scan_progress.bind_on_cancel(self._cancel_active_scan)

        # Bind window close event to save settings
        self.root.protocol("WM_DELETE_WINDOW", self._on_closing)

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
        """Kick off an asynchronous scan for the selected source folder."""
        if not self.source_folder:
            return

        self.scan_controller.scan(
            self.source_folder,
            on_started=self._on_scan_started,
            on_success=self._on_scan_success,
            on_error=self._on_scan_error,
            on_progress=self._on_scan_progress,
            on_cancelled=self._on_scan_cancelled,
        )

    def _on_scan_started(self) -> None:
        """Prepare UI for a background scan."""
        self._set_status("Scanning files...")
        self.scan_progress.start()
        self.date_list.populate({})
        self.preview_widget.clear_previews()
        self._update_execute_button_state()

    def _on_scan_success(
        self,
        folder_path: Path,
        files_by_date: dict[str, list[Path]],
    ) -> None:
        """Handle successful scan results."""
        self._populate_date_list()
        self._set_status(f"Found {len(files_by_date)} date groups with files")
        self.scan_progress.mark_finished(
            f"Scan complete ({len(files_by_date)} groups)"
        )
        self._update_execute_button_state()

    def _on_scan_error(self, error_msg: str) -> None:
        """Handle scan failure."""
        messagebox.showerror("Error", f"Error scanning folder: {error_msg}")
        self._set_status("Error scanning folder")
        self.scan_progress.show_error("Scan failed")
        self._update_execute_button_state()

    def _populate_date_list(self) -> None:
        """Populate the date listbox."""
        date_groups = self.file_manager.get_date_groups()
        date_counts = {date: self.file_manager.get_file_count(date) for date in date_groups}
        self.date_list.populate(date_counts)

    def _on_scan_progress(self, current: int, total: int) -> None:
        """Handle progress updates from the scan controller."""
        self.scan_progress.update_progress(current, total)
        if total:
            self._set_status(f"Scanning files... {current} / {total}")
        else:
            self._set_status("Scanning files...")

    def _on_scan_cancelled(self) -> None:
        """Handle scan cancellation callback."""
        self.scan_progress.mark_cancelled()
        self._set_status("Scan cancelled")
        self._populate_date_list()
        self._update_execute_button_state()

    def _cancel_active_scan(self) -> bool:
        """Request cancellation of the active scan."""
        cancelled = self.scan_controller.cancel_current_scan()
        if cancelled:
            self._set_status("Cancelling scan...")
        return cancelled

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
        self.execute_button.config(state='disabled')

        # Capture groups for potential restoration on cancellation/error
        pending_groups = {
            date_str: list(self.file_manager.get_files_for_date(date_str))
            for date_str in selected_dates
        }

        # Remove date groups from list IMMEDIATELY (before copy starts)
        for date_str in selected_dates:
            self.file_manager.remove_date_group(date_str)

        self._populate_date_list()
        self.preview_widget.clear_previews()

        self._pending_groups[custom_name] = pending_groups

        self.job_controller.start_job(
            job_name=custom_name,
            files=all_files,
            target_folder=self.target_folder,
            on_status=self._set_status,
            on_completed=self._on_job_completed,
            on_failed=self._on_job_failed,
            on_cancelled=self._on_job_cancelled,
        )

        # Clear selection for next operation
        self.custom_name_var.set("")

    def _on_job_completed(self, job_name: str) -> None:
        """Handle successful completion callbacks."""
        self._pending_groups.pop(job_name, None)
        self._update_execute_button_state()

    def _on_job_failed(self, job_name: str, error_msg: str) -> None:
        """Handle job failure callbacks."""
        self._restore_pending_groups(job_name)
        messagebox.showerror("Error", f"Copy failed for {job_name}:\n{error_msg}")
        self._update_execute_button_state()

    def _on_job_cancelled(self, job_name: str) -> None:
        """Handle job cancellation callbacks."""
        self._restore_pending_groups(job_name)
        self._update_execute_button_state()

    def _restore_pending_groups(self, job_name: str) -> None:
        """Reapply removed date groups when a job is cancelled or fails."""
        pending = self._pending_groups.pop(job_name, None)
        if not pending:
            return

        for date_str, paths in pending.items():
            self.file_manager.files_by_date[date_str] = paths

        self.file_manager.files_by_date = dict(sorted(self.file_manager.files_by_date.items()))
        self._populate_date_list()

    def _set_status(self, message: str) -> None:
        """Update the status bar text."""
        self.status_label.config(text=message)

    def _on_closing(self) -> None:
        """Handle window closing event."""
        # Save window geometry
        self.settings.window_geometry = self.root.geometry()
        self.root.destroy()
