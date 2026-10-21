"""Compact, scrollable background jobs with a bounded session history."""

from __future__ import annotations

import os
import tkinter as tk
from dataclasses import dataclass
from pathlib import Path
from tkinter import ttk
from collections.abc import Callable


@dataclass
class ProgressEntry:
    frame: ttk.Frame
    label: ttk.Label
    progress_bar: ttk.Progressbar
    total: int
    cancel_button: ttk.Button | None = None
    terminal: bool = False
    folder: Path | None = None
    message: str = ""
    current: int = 0
    waiting_reason: str | None = None


class ProgressManager:
    """Bound the panel height regardless of the number of imports."""

    def __init__(self, parent: ttk.Frame) -> None:
        self.parent = parent
        self.progress_bars: dict[str, ProgressEntry] = {}
        self._cancel_callbacks: dict[str, Callable[[], bool]] = {}
        self._history: list[tuple[str, str, Path | None]] = []
        self._history_window = None
        header = ttk.Frame(parent)
        header.pack(fill="x", pady=(0, 6))
        ttk.Label(header, text="Aufträge", style="Heading.TLabel").pack(side="left")
        self.summary_label = ttk.Label(
            header, text="Keine aktiven Aufträge", style="Small.TLabel"
        )
        self.summary_label.pack(side="left", padx=18)
        ttk.Button(
            header, text="Verlauf", style="Link.TButton", command=self.show_history
        ).pack(side="right")
        self.canvas = tk.Canvas(parent, height=76, bg="white", highlightthickness=0)
        scrollbar = ttk.Scrollbar(parent, command=self.canvas.yview)
        self.canvas.configure(yscrollcommand=scrollbar.set)
        scrollbar.pack(side="right", fill="y")
        self.canvas.pack(fill="x", expand=True)
        self.rows = ttk.Frame(self.canvas)
        self._window = self.canvas.create_window((0, 0), window=self.rows, anchor="nw")
        self.canvas.bind(
            "<Configure>",
            lambda e: self.canvas.itemconfigure(self._window, width=e.width),
        )
        self.rows.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all")),
        )
        self.canvas.bind("<MouseWheel>", self._wheel)

    def _wheel(self, event):
        self.canvas.yview_scroll(-int(event.delta / 120), "units")
        return "break"

    def add_progress_bar(
        self,
        job_name: str,
        total_files: int,
        on_cancel: Callable[[], bool] | None = None,
    ) -> None:
        self.remove_progress_bar(job_name)
        frame = ttk.Frame(self.rows, padding=(0, 3))
        frame.columnconfigure(1, weight=1)
        label = ttk.Label(
            frame,
            text=f"{job_name}: 0 / {total_files}",
            width=46,
            style="Small.TLabel",
            anchor="w",
        )
        label.grid(row=0, column=0, padx=(0, 16), sticky="w")
        bar = ttk.Progressbar(frame, mode="determinate", maximum=max(1, total_files))
        bar.grid(row=0, column=1, sticky="ew", padx=(0, 16))
        button = ttk.Button(
            frame, text="Abbrechen", command=lambda: self._handle_cancel(job_name)
        )
        button.grid(row=0, column=2, sticky="e")
        self.progress_bars[job_name] = ProgressEntry(
            frame, label, bar, total_files, button
        )
        if on_cancel:
            self._cancel_callbacks[job_name] = on_cancel
        else:
            button.config(state="disabled")
        for widget in (frame, label, bar, button):
            widget.bind("<MouseWheel>", self._wheel)
        self._refresh()

    def set_target(self, job_name: str, folder: Path) -> None:
        if job_name in self.progress_bars:
            self.progress_bars[job_name].folder = folder

    def update_progress(self, job_name: str, current: int, total: int) -> None:
        entry = self.progress_bars.get(job_name)
        if entry and not entry.terminal:
            entry.total = total
            entry.current = current
            entry.progress_bar.config(maximum=max(total, 1), value=current)
            self._render_progress(job_name, entry)

    def set_waiting(self, job_name: str, reason: str | None) -> None:
        """Show cooperative pauses without changing progress or cancel controls."""
        entry = self.progress_bars.get(job_name)
        if entry and not entry.terminal:
            entry.waiting_reason = reason
            self._render_progress(job_name, entry)

    def _render_progress(self, job_name: str, entry: ProgressEntry) -> None:
        if entry.waiting_reason:
            entry.label.config(text=f"{job_name} · {entry.waiting_reason}")
        else:
            phase = "konvertiert" if job_name.startswith("DNG: ") else "kopiert"
            entry.label.config(
                text=f"{job_name} · {entry.current} / {entry.total} {phase}"
            )

    def finish_progress(self, job_name: str, message: str) -> None:
        entry = self.progress_bars.get(job_name)
        if not entry:
            return
        entry.terminal = True
        entry.message = message
        translated = (
            message.replace("completed", "Abgeschlossen")
            .replace("failed", "Fehlgeschlagen")
            .replace("cancelled", "Abgebrochen")
        )
        entry.label.config(text=f"{job_name} · {translated.split(':', 1)[0]}")
        entry.progress_bar.grid_remove()
        self._cancel_callbacks.pop(job_name, None)
        entry.cancel_button.config(
            text="Ausblenden",
            state="normal",
            command=lambda: self.remove_progress_bar(job_name),
        )
        if entry.folder:
            ttk.Button(
                entry.frame,
                text="Ordner öffnen",
                style="Link.TButton",
                command=lambda: os.startfile(entry.folder),
            ).grid(row=0, column=1, sticky="e", padx=12)
        self._history.append((job_name, message, entry.folder))
        self._history = self._history[-100:]
        # Keep just two terminal widget rows; full results remain in history.
        terminal = [name for name, row in self.progress_bars.items() if row.terminal]
        for name in terminal[:-2]:
            self.remove_progress_bar(name)
        self._refresh()

    def _refresh(self) -> None:
        for entry in self.progress_bars.values():
            entry.frame.pack_forget()
        for terminal in (False, True):
            for entry in self.progress_bars.values():
                if entry.terminal == terminal:
                    entry.frame.pack(fill="x")
        active = sum(not row.terminal for row in self.progress_bars.values())
        self.summary_label.config(text=f"{len(self._history)} beendet · {active} aktiv")

    def mark_cancelling(self, job_name: str) -> None:
        entry = self.progress_bars.get(job_name)
        if entry and entry.cancel_button:
            entry.cancel_button.config(state="disabled", text="Wird abgebrochen …")

    def mark_cancel_failed(self, job_name: str) -> None:
        entry = self.progress_bars.get(job_name)
        if entry and entry.cancel_button:
            entry.cancel_button.config(state="normal", text="Abbrechen")

    def remove_progress_bar(self, job_name: str) -> None:
        entry = self.progress_bars.pop(job_name, None)
        if entry:
            entry.frame.destroy()
        self._cancel_callbacks.pop(job_name, None)
        self._refresh()

    def has_progress_bar(self, job_name: str) -> bool:
        return job_name in self.progress_bars

    def clear_all(self) -> None:
        for name in list(self.progress_bars):
            self.remove_progress_bar(name)

    def _handle_cancel(self, job_name: str) -> None:
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

    def show_history(self) -> None:
        """Show the latest 100 session results without blocking the main window."""
        if self._history_window and self._history_window.winfo_exists():
            self._history_window.destroy()
        window = self._history_window = tk.Toplevel(self.parent)
        window.title("Auftragsverlauf · Foto-Importer")
        window.geometry("850x500")
        window.columnconfigure(0, weight=1)
        window.rowconfigure(0, weight=1)
        tree = ttk.Treeview(window, columns=("result",), show="tree headings")
        tree.heading("#0", text="Ordner")
        tree.heading("result", text="Ergebnis")
        tree.column("#0", width=280)
        tree.column("result", width=520)
        tree.grid(row=0, column=0, sticky="nsew")
        scroll = ttk.Scrollbar(window, command=tree.yview)
        scroll.grid(row=0, column=1, sticky="ns")
        tree.config(yscrollcommand=scroll.set)
        details = tk.Text(
            window,
            height=5,
            wrap="word",
            font=("Segoe UI", 10),
            relief="flat",
            padx=12,
            pady=10,
            state="disabled",
        )
        details.grid(row=1, column=0, columnspan=2, sticky="ew")
        open_button = ttk.Button(window, text="Ordner öffnen", state="disabled")
        open_button.grid(row=2, column=0, sticky="e", padx=12, pady=12)
        results = list(reversed(self._history))
        for index, (name, message, folder) in enumerate(results):
            tree.insert("", "end", iid=str(index), text=name, values=(message,))

        def selected(event=None):
            selection = tree.selection()
            if not selection:
                return
            name, message, folder = results[int(selection[0])]
            details.config(state="normal")
            details.delete("1.0", "end")
            details.insert("1.0", f"{name}\n{message}")
            details.config(state="disabled")
            open_button.config(
                state="normal" if folder else "disabled",
                command=lambda: os.startfile(folder),
            )

        tree.bind("<<TreeviewSelect>>", selected)
        if results:
            tree.selection_set("0")
