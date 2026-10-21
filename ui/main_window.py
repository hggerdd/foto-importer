"""Main application window for camera organizer."""

from __future__ import annotations

import tkinter as tk
from pathlib import Path
from tkinter import messagebox

from config.settings import Settings
from core.copy_worker import CopyWorker
from core.task_scheduler import TaskScheduler
from core.file_manager import FileManager, DateSource
from core.import_plan import SelectionSummary, folder_name_error
from ui.dates import date_label
from ui.selection_controller import SelectionController
from ui.import_preflight import ImportPreflight
from ui.controllers import CopyJobController, SourceScanController
from ui.dng_controller import DngController
from ui.dng_update_controller import DngUpdateController
from ui.layout import MainLayout


class MainWindow:
    """Main application window."""

    def __init__(self, root: tk.Tk) -> None:
        """Initialize main window.

        Args:
            root: Root Tk instance
        """
        self.root: tk.Tk = root
        self.root.title("Foto-Importer")

        # Initialize settings
        self.settings: Settings = Settings()
        self.root.geometry(self.settings.window_geometry)

        # Initialize managers
        self.file_manager: FileManager = FileManager()
        self.file_manager.set_date_source(
            DateSource.from_value(self.settings.date_source)
        )
        self.task_scheduler = TaskScheduler()
        self.copy_worker: CopyWorker = CopyWorker(self.task_scheduler)

        # State
        self.source_folder: Path | None = None
        self.target_folder: Path | None = (
            Path(self.settings.last_target_folder)
            if self.settings.last_target_folder
            else None
        )
        self._selection_counts: dict[str, int] = {}
        self._preflight_pending = False
        self._source_revision = 0
        self._preview_day: str | None = None
        self._converter_after = None
        self._startup_after = None
        self._pending_groups: dict[str, dict[str, list[Path]]] = {}
        self._pending_sources: dict[str, Path | None] = {}

        # Setup UI layout and widgets
        self.layout = MainLayout(
            root=self.root,
            settings=self.settings,
            on_source_selected=self._on_source_selected,
            on_target_selected=self._on_target_selected,
            on_date_source_changed=self._on_date_source_changed,
            on_date_selection_changed=self._on_date_selection_changed,
            on_custom_name_changed=self._update_execute_button_state,
            on_execute_copy=self._execute_copy,
            task_scheduler=self.task_scheduler,
        )

        self.folder_selector = self.layout.folder_selector
        self.date_list = self.layout.date_list
        self.preview_widget = self.layout.preview_widget
        self.date_source_var = self.layout.date_source_var
        self.custom_name_var = self.layout.custom_name_var
        self.custom_name_entry = self.layout.custom_name_entry
        self.execute_button = self.layout.execute_button
        self.status_label = self.layout.status_label
        self.scan_progress = self.layout.scan_progress
        self.progress_manager = self.layout.progress_manager

        # Controllers for background tasks
        self.selection_controller = SelectionController(self.root, self.task_scheduler)
        self.import_preflight = ImportPreflight(self.root, self.task_scheduler)
        self.scan_controller = SourceScanController(
            self.root, self.file_manager, self.task_scheduler
        )
        self.job_controller = CopyJobController(
            self.root,
            self.copy_worker,
            self.progress_manager,
        )
        self.dng_controller = DngController(
            self.root, self.progress_manager, self.task_scheduler
        )
        self.dng_update_controller = DngUpdateController(
            self.root,
            self.settings,
            self._on_dng_version_checked,
        )
        self.dng_update_controller.start(Path(self.layout.dng_converter_var.get()))
        self.scan_progress.bind_on_cancel(self._cancel_active_scan)

        # Bind window close event to save settings
        self.root.protocol("WM_DELETE_WINDOW", self._on_closing)
        self.layout.dng_converter_var.trace_add("write", self._converter_changed)
        if self.settings.last_source_folder:
            self._startup_after = self.root.after(0, self._restore_source)

    def _restore_source(self) -> None:
        self._startup_after = None
        self._on_source_selected(Path(self.settings.last_source_folder))

    def _on_dng_version_checked(self, text: str, update_available: bool) -> None:
        """Show converter status independently of import progress."""
        text = (
            text.replace(
                "not found / version unavailable", "nicht gefunden / Version unbekannt"
            )
            .replace("Update available", "Update verfügbar")
            .replace("Up to date", "Aktuell")
            .replace(
                "Update check unavailable; next attempt tomorrow",
                "Updateprüfung nicht verfügbar; nächster Versuch morgen",
            )
        )
        self.layout.dng_version_label.config(text=text)
        version = text.split(" — ")[0].removeprefix("DNG Converter: ")
        self.layout.converter_status.config(text=version)
        self.layout.dng_download_button.config(
            state="normal" if update_available else "disabled",
        )

    def _converter_changed(self, *args) -> None:
        if self._converter_after:
            self.root.after_cancel(self._converter_after)
        self._converter_after = self.root.after(400, self._check_converter)

    def _check_converter(self) -> None:
        self._converter_after = None
        self.dng_update_controller.start(Path(self.layout.dng_converter_var.get()))

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

    def _on_date_source_changed(self) -> None:
        """Handle preferred date source changes."""
        selected_value = self.date_source_var.get()
        date_source = DateSource.from_value(selected_value)
        if self.settings.date_source != date_source.value:
            self.settings.date_source = date_source.value
        self.file_manager.set_date_source(date_source)

        if self.source_folder:
            self._scan_source_folder()
        else:
            self.scan_progress.reset()
            self._set_status("Bereit")

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
        self._source_revision += 1
        self.selection_controller.cancel()
        self._selection_counts = {}
        self._preview_day = None
        self.layout.preview_title.config(text="Deine Aufnahmen")
        self.layout.preview_details.config(text="Quelle wird gelesen …")
        self.layout.preview_caption.config(text="Bildvorschau")
        self._set_status("Dateien werden gelesen …")
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
        self._set_status(f"{len(files_by_date)} Aufnahmetage gefunden")
        self.scan_progress.mark_finished(
            f"✓ Scan abgeschlossen · {len(files_by_date)} Aufnahmetage"
        )
        self.layout.preview_details.config(text="Links einen Aufnahmetag auswählen")
        self._update_execute_button_state()

    def _on_scan_error(self, error_msg: str) -> None:
        """Handle scan failure."""
        messagebox.showerror(
            "Fehler", f"Quelle konnte nicht gelesen werden: {error_msg}"
        )
        self._set_status("Fehler beim Lesen der Quelle")
        self.scan_progress.show_error("Scan fehlgeschlagen")
        self._update_execute_button_state()

    def _populate_date_list(self) -> None:
        """Populate the date listbox."""
        date_groups = self.file_manager.get_date_groups()
        date_counts = {
            date: self.file_manager.get_file_count(date) for date in date_groups
        }
        self.date_list.populate(date_counts)

    def _on_scan_progress(self, current: int, total: int) -> None:
        """Handle progress updates from the scan controller."""
        self.scan_progress.update_progress(current, total)
        if total:
            self._set_status(f"Dateien werden gelesen … {current} / {total}")
        else:
            self._set_status("Dateien werden gelesen …")

    def _on_scan_cancelled(self) -> None:
        """Handle scan cancellation callback."""
        self.scan_progress.mark_cancelled()
        self._set_status("Scan abgebrochen")
        if self.file_manager.source_folder == self.source_folder:
            self._populate_date_list()
        else:
            self.date_list.populate({})
        self._update_execute_button_state()

    def _cancel_active_scan(self) -> bool:
        """Request cancellation of the active scan."""
        cancelled = self.scan_controller.cancel_current_scan()
        if cancelled:
            self._set_status("Scan wird abgebrochen …")
        return cancelled

    def _on_date_selection_changed(self, selected_dates: set[str]) -> None:
        """Compute summaries off-thread and preview the focused day deterministically."""
        focused = self.date_list.get_first_selected_date()
        self._selection_counts = {}
        self._update_execute_button_state()
        if focused != self._preview_day:
            self.preview_widget.clear_previews()
            self._preview_day = focused
        self.layout.preview_title.config(
            text=date_label(focused) if focused else "Deine Aufnahmen"
        )
        self.layout.preview_details.config(
            text="Dateien werden gezählt …"
            if focused
            else "Links einen Aufnahmetag auswählen"
        )
        self.selection_controller.request(
            self.file_manager.files_by_date,
            selected_dates,
            focused,
            lambda result: self._selection_ready(focused, result),
        )

    def _selection_ready(self, focused: str | None, result: SelectionSummary) -> None:
        self._selection_counts = result.counts
        details = " · ".join(
            f"{count} {ext.upper()}"
            for ext, count in sorted(result.focused_counts.items())
        )
        self.layout.preview_details.config(text=details or "Keine Dateien ausgewählt")
        self.layout.preview_caption.config(
            text=f"Bildvorschau · {result.preview_total} Bilder"
        )
        self.preview_widget.load_previews(
            result.previews,
            empty_message=(
                "Für diese Dateien ist keine Bildvorschau verfügbar.\n"
                "RAW-Dateien und Videos werden trotzdem importiert."
                if result.focused_counts
                else "Aufnahmetag auswählen\nDie Bildvorschau erscheint hier."
            ),
        )
        if focused and not self.custom_name_var.get():
            self.custom_name_var.set(f"{focused}_")
        self._update_execute_button_state()

    def _show_preview(self, date_str: str) -> None:
        """Refresh previews using the asynchronous selection pipeline."""
        self._on_date_selection_changed(self.date_list.get_selected_dates())

    def _update_execute_button_state(self) -> None:
        """Update cheap in-memory UI state; never probe a drive here."""
        selected = self.date_list.get_selected_dates()
        count = sum(self.file_manager.get_file_count(day) for day in selected)
        name = self.custom_name_var.get().strip()
        error = folder_name_error(name)
        active = name in self._pending_groups or (
            hasattr(self, "dng_controller") and self.dng_controller.is_active(name)
        )
        ready = bool(
            count
            and self.target_folder
            and not error
            and not active
            and not self._preflight_pending
        )
        self.execute_button.config(
            state="normal" if ready else "disabled",
            text="Ziel wird geprüft …"
            if self._preflight_pending
            else f"{count} {'Datei' if count == 1 else 'Dateien'} importieren →"
            if count
            else "Dateien importieren →",
        )
        self.layout.selection_summary.config(
            text=f"{len(selected)} Tag{'e' if len(selected) != 1 else ''} ausgewählt · {count} Dateien"
            if selected
            else "Kein Tag ausgewählt"
        )
        self.layout.import_hint.config(
            text=(
                error
                if name and error
                else "Für diesen Ordner läuft bereits ein Auftrag."
                if active
                else "Bitte einen Zielordner auswählen."
                if not self.target_folder
                else "Dateien werden kopiert."
            )
        )
        self.layout.update_destination(self._selection_counts)

    def _execute_copy(self) -> None:
        """Execute the copy operation."""
        selected_dates = self.date_list.get_selected_dates()

        if not selected_dates:
            messagebox.showwarning(
                "Auswahl fehlt", "Bitte mindestens einen Aufnahmetag auswählen."
            )
            return

        if not self.target_folder:
            messagebox.showwarning("Ziel fehlt", "Bitte einen Zielordner auswählen.")
            return

        custom_name = self.custom_name_var.get().strip()
        if not custom_name:
            messagebox.showwarning(
                "Ordnername fehlt", "Bitte einen Ordnernamen eingeben."
            )
            return

        if custom_name in self._pending_groups or self.dng_controller.is_active(
            custom_name
        ):
            messagebox.showwarning(
                "Auftrag läuft", f"Für '{custom_name}' läuft bereits ein Auftrag."
            )
            return

        name_error = folder_name_error(custom_name)
        if name_error:
            messagebox.showwarning("Ordnername", name_error)
            return
        if self._preflight_pending:
            return
        converter = (
            Path(self.layout.dng_converter_var.get().strip())
            if self.layout.convert_to_dng_var.get()
            else None
        )
        target_folder = self.target_folder
        target_path = target_folder / custom_name
        pending_groups = {
            day: self.file_manager.get_files_for_date(day) for day in selected_dates
        }
        revision = self._source_revision
        self._preflight_pending = True
        self._update_execute_button_state()

        def checked(exists: bool, error: str | None, files: list[Path]) -> None:
            self._preflight_pending = False
            self._update_execute_button_state()
            if revision != self._source_revision:
                self._set_status("Quelle geändert. Bitte den Import erneut starten.")
                return
            if error:
                messagebox.showerror("Import nicht möglich", error)
                return
            if exists and not messagebox.askyesno(
                "Ordner vorhanden",
                f"Der Ordner '{custom_name}' existiert bereits.\n\n"
                "Dateien werden ergänzt; vorhandene Dateien bleiben erhalten.\nFortfahren?",
            ):
                return
            self._start_copy(
                custom_name, pending_groups, target_folder, converter, files
            )

        self.import_preflight.check(target_path, converter, pending_groups, checked)

    def _start_copy(
        self,
        custom_name: str,
        pending_groups: dict[str, list[Path]],
        target_folder: Path,
        converter: Path | None,
        all_files: list[Path],
    ) -> None:
        """Start the captured import after background validation."""
        if not all_files:
            return
        target_path = target_folder / custom_name
        selected_dates = set(pending_groups)
        # Start copy operation
        self.execute_button.config(state="disabled")

        # Remove date groups from list IMMEDIATELY (before copy starts)
        for date_str in selected_dates:
            self.file_manager.remove_date_group(date_str)

        self._populate_date_list()
        self.selection_controller.cancel()
        self._preview_day = None
        self._selection_counts = {}
        self.preview_widget.clear_previews()
        self.layout.preview_title.config(text="Deine Aufnahmen")
        self.layout.preview_details.config(text="Nächsten Aufnahmetag auswählen")
        self.layout.preview_caption.config(text="Bildvorschau")

        self._pending_groups[custom_name] = pending_groups
        self._pending_sources[custom_name] = self.source_folder

        self.job_controller.start_job(
            job_name=custom_name,
            files=all_files,
            target_folder=target_folder,
            on_status=self._set_status,
            on_completed=lambda name: self._on_copy_completed(
                name,
                target_path,
                converter,
            ),
            on_failed=self._on_job_failed,
            on_cancelled=self._on_job_cancelled,
        )

        self.progress_manager.set_target(custom_name, target_path)

        # Clear selection for next operation
        if self.custom_name_var.get().strip() == custom_name:
            self.custom_name_var.set("")
        self._on_date_selection_changed(self.date_list.get_selected_dates())

    def _on_copy_completed(
        self,
        job_name: str,
        folder: Path,
        converter: Path | None,
    ) -> None:
        """Commit copy success before starting the optional conversion phase."""
        self._on_job_completed(job_name)
        if converter is not None:
            self.progress_manager.remove_progress_bar(job_name)
            self.dng_controller.start_job(
                job_name, folder, converter, self._on_dng_status
            )
            self.progress_manager.set_target(f"DNG: {job_name}", folder)

    def _on_dng_status(self, message: str) -> None:
        """Reflect phase transitions in the import action's availability."""
        self._set_status(message)
        self._update_execute_button_state()

    def _on_job_completed(self, job_name: str) -> None:
        """Handle successful completion callbacks."""
        self._pending_groups.pop(job_name, None)
        self._pending_sources.pop(job_name, None)
        self._update_execute_button_state()

    def _on_job_failed(self, job_name: str, error_msg: str) -> None:
        """Handle job failure callbacks."""
        self._restore_pending_groups(job_name)
        messagebox.showerror(
            "Fehler", f"Kopieren fehlgeschlagen für {job_name}:\n{error_msg}"
        )
        self._update_execute_button_state()

    def _on_job_cancelled(self, job_name: str) -> None:
        """Handle job cancellation callbacks."""
        self._restore_pending_groups(job_name)
        self._update_execute_button_state()

    def _restore_pending_groups(self, job_name: str) -> None:
        """Reapply removed date groups when a job is cancelled or fails."""
        pending = self._pending_groups.pop(job_name, None)
        source = self._pending_sources.pop(job_name, None)
        if not pending or source != self.source_folder:
            return

        for date_str, paths in pending.items():
            self.file_manager.files_by_date[date_str] = paths

        self.file_manager.files_by_date = dict(
            sorted(self.file_manager.files_by_date.items())
        )
        self._populate_date_list()

    def _set_status(self, message: str) -> None:
        """Update the status bar text."""
        self.status_label.config(text=message)

    def _on_closing(self) -> None:
        """Handle window closing event."""
        # Save window geometry
        self.settings.window_geometry = self.root.geometry()
        for token in (self._converter_after, self._startup_after):
            if token:
                self.root.after_cancel(token)
        self.selection_controller.close()
        self.import_preflight.close()
        self.scan_controller.close()
        self.job_controller.close()
        self.dng_controller.close()
        self.dng_update_controller.close()
        self.root.destroy()
