"""Helper controllers for coordinating UI background tasks."""
from __future__ import annotations

import threading
import tkinter as tk
from pathlib import Path
from typing import Callable

from core.copy_worker import CopyWorker, JobState
from core.file_manager import FileManager
from ui.progress_manager import ProgressManager


class SourceScanController:
    """Runs source-folder scans on a worker thread and marshals results."""

    def __init__(self, root: tk.Misc, file_manager: FileManager) -> None:
        self._root = root
        self._file_manager = file_manager
        self._request_lock = threading.Lock()
        self._request_id = 0

    def scan(
        self,
        folder_path: Path,
        on_started: Callable[[], None],
        on_success: Callable[[Path, dict[str, list[Path]]], None],
        on_error: Callable[[str], None],
    ) -> None:
        """Kick off an asynchronous scan for the provided folder."""
        with self._request_lock:
            self._request_id += 1
            request_id = self._request_id

        if on_started:
            on_started()

        def worker() -> None:
            try:
                files_by_date = self._file_manager.gather_files_by_date(folder_path)
            except Exception as exc:  # noqa: BLE001 - surface to UI
                self._root.after(
                    0, self._handle_error, request_id, str(exc), on_error
                )
                return

            self._root.after(
                0,
                self._handle_success,
                request_id,
                folder_path,
                files_by_date,
                on_success,
            )

        threading.Thread(target=worker, daemon=True).start()

    def _handle_success(
        self,
        request_id: int,
        folder_path: Path,
        files_by_date: dict[str, list[Path]],
        callback: Callable[[Path, dict[str, list[Path]]], None],
    ) -> None:
        if request_id != self._request_id:
            return

        self._file_manager.apply_scan_results(folder_path, files_by_date)
        if callback:
            callback(folder_path, files_by_date)

    def _handle_error(
        self,
        request_id: int,
        error_msg: str,
        callback: Callable[[str], None],
    ) -> None:
        if request_id != self._request_id:
            return

        if callback:
            callback(error_msg)


class CopyJobController:
    """Coordinates copy-worker jobs and progress updates for the UI."""

    def __init__(
        self,
        root: tk.Misc,
        copy_worker: CopyWorker,
        progress_manager: ProgressManager,
    ) -> None:
        self._root = root
        self._copy_worker = copy_worker
        self._progress_manager = progress_manager
        self._active_jobs: set[str] = set()

    def start_job(
        self,
        job_name: str,
        files: list[Path],
        target_folder: Path,
        *,
        on_status: Callable[[str], None],
        on_completed: Callable[[str], None],
        on_failed: Callable[[str, str], None],
        on_cancelled: Callable[[str], None],
    ) -> None:
        """Start a new copy job and wire up UI callbacks."""
        if job_name in self._active_jobs:
            raise ValueError(f"Copy job '{job_name}' is already running")

        total_files = len(files)
        self._active_jobs.add(job_name)
        self._progress_manager.add_progress_bar(
            job_name,
            total_files,
            on_cancel=lambda: self.cancel_job(job_name, on_status),
        )
        on_status(f"Copying {total_files} files...")

        self._copy_worker.copy_files(
            files=files,
            target_folder=target_folder,
            custom_name=job_name,
            on_complete=lambda name: self._root.after(
                0, self._handle_complete, name, on_status, on_completed
            ),
            on_error=lambda name, err: self._root.after(
                0, self._handle_error, name, err, on_status, on_failed
            ),
            on_progress=lambda current, total: self._root.after(
                0, self._handle_progress, job_name, current, total
            ),
            on_status_change=lambda name, state: self._root.after(
                0,
                self._handle_status_change,
                name,
                state,
                on_status,
                on_cancelled,
            ),
        )

    def cancel_job(
        self,
        job_name: str,
        on_status: Callable[[str], None],
    ) -> bool:
        """Request cancellation of a running job."""
        success = self._copy_worker.cancel_job(job_name)
        if success:
            on_status(f"Cancelling job: {job_name}")
        else:
            on_status(f"Unable to cancel job: {job_name}")
        return success

    def _handle_progress(self, job_name: str, current: int, total: int) -> None:
        self._progress_manager.update_progress(job_name, current, total)

    def _handle_complete(
        self,
        job_name: str,
        on_status: Callable[[str], None],
        on_completed: Callable[[str], None],
    ) -> None:
        self._progress_manager.remove_progress_bar(job_name)
        self._active_jobs.discard(job_name)
        on_status(f"Copy completed: {job_name}")
        on_completed(job_name)

    def _handle_error(
        self,
        job_name: str,
        error_msg: str,
        on_status: Callable[[str], None],
        on_failed: Callable[[str, str], None],
    ) -> None:
        self._progress_manager.remove_progress_bar(job_name)
        self._active_jobs.discard(job_name)
        on_status(f"Copy failed: {error_msg}")
        on_failed(job_name, error_msg)

    def _handle_status_change(
        self,
        job_name: str,
        state: JobState,
        on_status: Callable[[str], None],
        on_cancelled: Callable[[str], None],
    ) -> None:
        if state is JobState.RUNNING:
            on_status(f"Copy in progress: {job_name}")
            return

        if state is JobState.CANCELLED:
            self._progress_manager.remove_progress_bar(job_name)
            self._active_jobs.discard(job_name)
            on_status(f"Copy cancelled: {job_name}")
            on_cancelled(job_name)
