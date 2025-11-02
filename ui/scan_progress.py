"""Widget for showing source scan progress."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from collections.abc import Callable


class ScanProgressWidget(ttk.LabelFrame):
    """Displays progress for source-folder scans with cancellation control."""

    def __init__(self, parent: ttk.Frame) -> None:
        super().__init__(parent, text="Source Scan", padding="10")
        self.status_var = tk.StringVar(value="Idle")
        self._cancel_callback: Callable[[], bool] | None = None
        self._indeterminate = False

        self._build_ui()
        self.reset()

    def _build_ui(self) -> None:
        """Build the widget layout."""
        self.columnconfigure(0, weight=1)

        status_label = ttk.Label(self, textvariable=self.status_var)
        status_label.grid(row=0, column=0, sticky=(tk.W, tk.E))

        self.progress_bar = ttk.Progressbar(self, mode='determinate', maximum=1)
        self.progress_bar.grid(row=1, column=0, sticky=(tk.W, tk.E), pady=(5, 5))

        self.cancel_button = ttk.Button(self, text="Cancel Scan", command=self._handle_cancel)
        self.cancel_button.grid(row=2, column=0, sticky=tk.W)

    def bind_on_cancel(self, callback: Callable[[], bool]) -> None:
        """Bind a cancellation callback returning success state."""
        self._cancel_callback = callback

    def start(self, total: int | None = None) -> None:
        """Prepare the widget for a new scan."""
        self._switch_mode(total)
        self.progress_bar['value'] = 0
        self.status_var.set("Scanning files...")
        self.cancel_button.config(state='normal', text="Cancel Scan")

    def update_progress(self, current: int, total: int) -> None:
        """Update progress bar based on scan updates."""
        if self._indeterminate:
            self._switch_mode(total)

        maximum = max(total, 1)
        self.progress_bar.config(maximum=maximum)
        self.progress_bar['value'] = min(current, maximum)
        self.status_var.set(f"Scanning files... {current} / {total}")

    def mark_cancelling(self) -> None:
        """Set UI state to reflect an in-flight cancellation."""
        self.cancel_button.config(state='disabled', text="Cancelling...")
        self.status_var.set("Cancelling scan...")

    def mark_cancel_failed(self) -> None:
        """Restore cancel button if cancellation was rejected."""
        self.cancel_button.config(state='normal', text="Cancel Scan")
        self.status_var.set("Unable to cancel scan")

    def mark_cancelled(self) -> None:
        """Reflect a cancelled scan."""
        self.progress_bar.stop()
        self.status_var.set("Scan cancelled")
        self.cancel_button.config(state='disabled', text="Cancel Scan")
        self._indeterminate = False

    def mark_finished(self, message: str = "Scan complete") -> None:
        """Mark scan as finished and disable controls."""
        if self._indeterminate:
            self.progress_bar.stop()
        self.status_var.set(message)
        self.cancel_button.config(state='disabled', text="Cancel Scan")
        self._indeterminate = False

    def show_error(self, message: str) -> None:
        """Display an error message."""
        if self._indeterminate:
            self.progress_bar.stop()
        self.status_var.set(message)
        self.cancel_button.config(state='disabled', text="Cancel Scan")
        self._indeterminate = False

    def reset(self) -> None:
        """Reset to idle state."""
        self.progress_bar.stop()
        self.progress_bar.config(mode='determinate', maximum=1)
        self.progress_bar['value'] = 0
        self._indeterminate = False
        self.status_var.set("Idle")
        self.cancel_button.config(state='disabled', text="Cancel Scan")

    def _handle_cancel(self) -> None:
        """Invoke the bound cancellation callback."""
        if not self._cancel_callback:
            self.cancel_button.config(state='disabled')
            return

        self.mark_cancelling()
        success = False
        try:
            success = self._cancel_callback()
        finally:
            if not success:
                self.mark_cancel_failed()

    def _switch_mode(self, total: int | None) -> None:
        """Switch progress bar mode based on availability of total count."""
        if total and total > 0:
            if self._indeterminate:
                self.progress_bar.stop()
            self.progress_bar.config(mode='determinate', maximum=total)
            self._indeterminate = False
        else:
            if not self._indeterminate:
                self.progress_bar.config(mode='indeterminate')
                self.progress_bar.start(10)
                self._indeterminate = True
