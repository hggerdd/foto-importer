"""Progress bar manager for copy operations."""
from __future__ import annotations

import tkinter as tk
from dataclasses import dataclass
from tkinter import ttk
from typing import Callable


@dataclass
class ProgressEntry:
    """Holds widgets associated with a progress bar row."""

    frame: ttk.Frame
    label: ttk.Label
    progress_bar: ttk.Progressbar
    total: int
    cancel_button: ttk.Button | None = None


class ProgressManager:
    """Manages progress bars for background copy operations."""

    def __init__(self, parent: ttk.LabelFrame) -> None:
        """Initialize progress manager.

        Args:
            parent: Parent frame to contain progress bars
        """
        self.parent: ttk.LabelFrame = parent
        self.progress_bars: dict[str, ProgressEntry] = {}
        self._cancel_callbacks: dict[str, Callable[[], bool]] = {}

    def add_progress_bar(
        self,
        job_name: str,
        total_files: int,
        on_cancel: Callable[[], bool] | None = None
    ) -> None:
        """Add a progress bar for a copy job.

        Args:
            job_name: Name of the copy job
            total_files: Total number of files to copy
            on_cancel: Optional callback invoked when the cancel button is pressed.
        """
        job_frame = ttk.Frame(self.parent)
        job_frame.pack(fill=tk.X, pady=(0, 5))

        label = ttk.Label(job_frame, text=f"{job_name}: 0 / {total_files} files")
        label.pack(side=tk.LEFT, padx=(0, 10))

        progress_bar = ttk.Progressbar(job_frame, mode='determinate', maximum=total_files)
        progress_bar.pack(side=tk.LEFT, fill=tk.X, expand=True)

        cancel_button: ttk.Button | None = None
        if on_cancel:
            cancel_button = ttk.Button(job_frame, text="Cancel")
            cancel_button.pack(side=tk.RIGHT, padx=(10, 0))
            self._cancel_callbacks[job_name] = on_cancel
            cancel_button.config(command=lambda j=job_name: self._handle_cancel(j))

        self.progress_bars[job_name] = ProgressEntry(
            frame=job_frame,
            label=label,
            progress_bar=progress_bar,
            total=total_files,
            cancel_button=cancel_button
        )

    def update_progress(self, job_name: str, current: int, total: int) -> None:
        """Update progress for a copy job."""
        entry = self.progress_bars.get(job_name)
        if not entry:
            return

        entry.label.config(text=f"{job_name}: {current} / {entry.total} files")
        entry.progress_bar['value'] = current

    def mark_cancelling(self, job_name: str) -> None:
        """Mark a job as pending cancellation (disables the cancel button)."""
        entry = self.progress_bars.get(job_name)
        if not entry or not entry.cancel_button:
            return
        entry.cancel_button.config(state='disabled', text='Cancelling...')

    def mark_cancel_failed(self, job_name: str) -> None:
        """Re-enable cancel controls if cancellation was rejected."""
        entry = self.progress_bars.get(job_name)
        if not entry or not entry.cancel_button:
            return
        entry.cancel_button.config(state='normal', text='Cancel')

    def remove_progress_bar(self, job_name: str) -> None:
        """Remove progress bar after job completion."""
        entry = self.progress_bars.pop(job_name, None)
        if not entry:
            return

        entry.frame.destroy()
        self._cancel_callbacks.pop(job_name, None)

    def has_progress_bar(self, job_name: str) -> bool:
        """Check if progress bar exists for a job."""
        return job_name in self.progress_bars

    def clear_all(self) -> None:
        """Clear all progress bars."""
        for job_name in list(self.progress_bars.keys()):
            self.remove_progress_bar(job_name)

    def _handle_cancel(self, job_name: str) -> None:
        """Invoke a cancellation callback and update the UI."""
        callback = self._cancel_callbacks.get(job_name)
        if not callback:
            return

        self.mark_cancelling(job_name)

        try:
            success = callback()
        except Exception:
            success = False

        if not success:
            self.mark_cancel_failed(job_name)
