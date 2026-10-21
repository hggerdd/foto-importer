"""Coordinate folder conversion jobs without calling Tk from workers."""
from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
import tkinter as tk

from core.copy_worker import JobState
from core.dng_worker import DngJob
from ui.dispatcher import UiDispatcher
from ui.progress_manager import ProgressManager


class DngController:
    """Own an independent converter job and progress row for every folder."""

    def __init__(self, root: tk.Misc, progress: ProgressManager) -> None:
        self._dispatcher = UiDispatcher(root)
        self._progress = progress
        self._jobs: dict[str, DngJob] = {}

    def is_active(self, name: str) -> bool:
        """Return whether the named import still has a conversion job."""
        return name in self._jobs

    def start_job(
        self, name: str, folder: Path, converter: Path,
        on_status: Callable[[str], None],
    ) -> None:
        """Start converting a copied folder and queue all UI updates."""
        if self.is_active(name):
            raise ValueError(f"DNG conversion already running: {name}")
        row = f"DNG: {name}"
        self._progress.remove_progress_bar(row)
        self._progress.add_progress_bar(row, 0, on_cancel=lambda: self._jobs[name].cancel())
        job = DngJob(
            folder, converter,
            on_progress=lambda current, total: self._dispatcher.post(
                self._progress.update_progress, row, current, total,
                key=("dng", name),
            ),
            on_finished=lambda state, message: self._dispatcher.post(
                self._finished, name, row, state, message, on_status,
            ),
        )
        self._jobs[name] = job
        on_status(f"Converting NEF to DNG: {name}")
        job.start()

    def _finished(
        self, name: str, row: str, state: JobState, message: str,
        on_status: Callable[[str], None],
    ) -> None:
        self._jobs.pop(name, None)
        result = f"{state.value}: {message}"
        self._progress.finish_progress(row, result)
        on_status(f"DNG {name}: {result}")

    def close(self) -> None:
        """Cancel child processes asynchronously and stop UI delivery."""
        for job in self._jobs.values():
            job.cancel()
        self._dispatcher.close()
