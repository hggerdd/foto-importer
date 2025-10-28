"""Date list widget for displaying and selecting date groups."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from collections.abc import Callable


class DateListWidget(ttk.LabelFrame):
    """Widget for displaying and selecting date groups."""

    def __init__(
        self,
        parent: ttk.Frame,
        on_selection_changed: Callable[[set[str]], None] | None = None
    ) -> None:
        """Initialize date list widget.

        Args:
            parent: Parent frame
            on_selection_changed: Callback when selection changes (receives set of selected dates)
        """
        super().__init__(parent, text="Date Groups", padding="10")

        self.on_selection_changed: Callable[[set[str]], None] | None = on_selection_changed

        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        # Scrollable listbox
        list_scroll = ttk.Scrollbar(self, orient='vertical')
        self.listbox = tk.Listbox(
            self,
            height=8,
            selectmode='extended',
            yscrollcommand=list_scroll.set
        )
        list_scroll.config(command=self.listbox.yview)

        self.listbox.grid(row=0, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))
        list_scroll.grid(row=0, column=1, sticky=(tk.N, tk.S))

        # Bind selection event
        self.listbox.bind('<<ListboxSelect>>', self._on_selection)

        self.columnconfigure(0, weight=1)
        self.rowconfigure(0, weight=1)

    def _on_selection(self, event: tk.Event) -> None:
        """Handle selection change."""
        selected_dates = self.get_selected_dates()
        if self.on_selection_changed:
            self.on_selection_changed(selected_dates)

    def populate(self, date_groups: dict[str, int]) -> None:
        """Populate the list with date groups.

        Args:
            date_groups: Dictionary mapping date strings to file counts
        """
        self.listbox.delete(0, tk.END)
        for date_str, file_count in date_groups.items():
            self.listbox.insert(tk.END, f"{date_str} ({file_count} files)")

    def get_selected_dates(self) -> set[str]:
        """Get the currently selected date strings.

        Returns:
            Set of selected date strings (YYYY-MM-DD format)
        """
        selection = self.listbox.curselection()
        selected_dates: set[str] = set()

        for idx in selection:
            date_text = self.listbox.get(idx)
            # Extract date from "YYYY-MM-DD (N files)"
            date_str = date_text.split(' ')[0]
            selected_dates.add(date_str)

        return selected_dates

    def get_first_selected_date(self) -> str | None:
        """Get the first selected date.

        Returns:
            First selected date string or None
        """
        selection = self.listbox.curselection()
        if not selection:
            return None

        date_text = self.listbox.get(selection[0])
        return date_text.split(' ')[0]

    def clear_selection(self) -> None:
        """Clear the current selection."""
        self.listbox.selection_clear(0, tk.END)

    def is_empty(self) -> bool:
        """Check if the list is empty.

        Returns:
            True if list is empty
        """
        return self.listbox.size() == 0
