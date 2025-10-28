"""Progress bar manager for copy operations."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk


class ProgressManager:
    """Manages progress bars for background copy operations."""

    def __init__(self, parent: ttk.LabelFrame) -> None:
        """Initialize progress manager.

        Args:
            parent: Parent frame to contain progress bars
        """
        self.parent: ttk.LabelFrame = parent
        self.progress_bars: dict[str, tuple[ttk.Frame, ttk.Label, ttk.Progressbar, int]] = {}

    def add_progress_bar(self, job_name: str, total_files: int) -> None:
        """Add a progress bar for a copy job.

        Args:
            job_name: Name of the copy job
            total_files: Total number of files to copy
        """
        # Create container for this job
        job_frame = ttk.Frame(self.parent)
        job_frame.pack(fill=tk.X, pady=(0, 5))

        # Label showing job name and progress
        label = ttk.Label(job_frame, text=f"{job_name}: 0 / {total_files} files")
        label.pack(side=tk.LEFT, padx=(0, 10))

        # Progress bar
        progress_bar = ttk.Progressbar(job_frame, mode='determinate', maximum=total_files)
        progress_bar.pack(side=tk.LEFT, fill=tk.X, expand=True)

        # Store references
        self.progress_bars[job_name] = (job_frame, label, progress_bar, total_files)

    def update_progress(self, job_name: str, current: int, total: int) -> None:
        """Update progress for a copy job.

        Args:
            job_name: Name of the copy job
            current: Current number of files copied
            total: Total number of files
        """
        if job_name in self.progress_bars:
            job_frame, label, progress_bar, total_files = self.progress_bars[job_name]
            label.config(text=f"{job_name}: {current} / {total} files")
            progress_bar['value'] = current

    def remove_progress_bar(self, job_name: str) -> None:
        """Remove progress bar after job completion.

        Args:
            job_name: Name of the copy job
        """
        if job_name in self.progress_bars:
            job_frame, label, progress_bar, total_files = self.progress_bars[job_name]
            job_frame.destroy()
            del self.progress_bars[job_name]

    def has_progress_bar(self, job_name: str) -> bool:
        """Check if progress bar exists for a job.

        Args:
            job_name: Name of the copy job

        Returns:
            True if progress bar exists
        """
        return job_name in self.progress_bars

    def clear_all(self) -> None:
        """Clear all progress bars."""
        for job_name in list(self.progress_bars.keys()):
            self.remove_progress_bar(job_name)
