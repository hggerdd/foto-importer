"""Scrollable day selection with independent preview focus and checkboxes."""

from __future__ import annotations

import tkinter as tk
from tkinter import ttk
from collections.abc import Callable

from ui.dates import date_label


class DateListWidget(ttk.Frame):
    """Use a native tree, keeping widget count constant for large scans."""

    def __init__(
        self,
        parent: tk.Misc,
        on_selection_changed: Callable[[set[str]], None] | None = None,
    ) -> None:
        super().__init__(parent, padding=14)
        self.on_selection_changed = on_selection_changed
        self._selected: set[str] = set()
        self._counts: dict[str, int] = {}
        self._imported_counts: dict[str, int] = {}
        self._focused: str | None = None
        self._populate_id = None
        header = ttk.Frame(self)
        header.pack(fill="x")
        ttk.Label(header, text="Aufnahmetage", style="Heading.TLabel").pack(side="left")
        self.select_all_button = ttk.Button(
            header, text="Alle auswählen", style="Link.TButton", command=self.toggle_all
        )
        self.select_all_button.pack(side="right")
        self.count_label = ttk.Label(
            self, text="Quelle auswählen", style="Muted.TLabel"
        )
        self.count_label.pack(anchor="w", pady=(3, 12))
        content = ttk.Frame(self)
        content.pack(fill="both", expand=True)
        self.tree = ttk.Treeview(
            content, columns=("count",), show="tree", selectmode="browse", height=8
        )
        self.tree.column("#0", width=205, minwidth=155, stretch=True)
        self.tree.column("count", width=62, minwidth=48, stretch=False, anchor="e")
        scrollbar = ttk.Scrollbar(content, command=self.tree.yview)
        self.tree.configure(yscrollcommand=scrollbar.set)
        self.tree.pack(side="left", fill="both", expand=True)
        scrollbar.pack(side="right", fill="y")
        self.tree.tag_configure(
            "month", foreground="#68717e", font=("Segoe UI", 9, "bold")
        )
        self.tree.tag_configure("imported", foreground="#89919b")
        self.tree.bind("<Button-1>", self._click)
        self.tree.bind("<space>", self._space)
        self.tree.bind("<<TreeviewSelect>>", self._focus_changed)
        self.footer = ttk.Frame(self)
        self.footer.pack(fill="x", pady=(12, 0))

    def populate(self, date_groups: dict[str, int]) -> None:
        """Replace rows while retaining checked days that still exist."""
        self._counts = date_groups
        self._imported_counts = {}
        if self._populate_id:
            self.after_cancel(self._populate_id)
            self._populate_id = None
        self._selected.intersection_update(date_groups)
        self._focused = self._focused if self._focused in date_groups else None
        self.tree.delete(*self.tree.get_children())
        items = iter(sorted(date_groups.items(), reverse=True))
        month = None

        def insert_batch():
            nonlocal month
            self._populate_id = None
            for _ in range(100):
                try:
                    day, count = next(items)
                except StopIteration:
                    return
                if day[:7] != month:
                    month = day[:7]
                    self.tree.insert(
                        "",
                        "end",
                        iid=f"month:{month}",
                        text=date_label(day).split(". ", 1)[1],
                        tags=("month",),
                    )
                self.tree.insert(
                    "", "end", iid=day, text=self._row_text(day), values=(str(count),)
                )
            self._populate_id = self.after(1, insert_batch)

        insert_batch()
        self.count_label.config(
            text=f"{len(date_groups)} Tage gefunden"
            if date_groups
            else "Keine Aufnahmen vorhanden"
        )
        self._update_all_label()

    def _row_text(self, day: str) -> str:
        text = f"{'☑' if day in self._selected else '☐'}  {date_label(day)}"
        if day in self._imported_counts:
            text += f" · {self._imported_counts[day]} von {self._counts[day]} importiert"
        return text

    def set_imported_counts(self, counts: dict[str, int]) -> None:
        self._imported_counts = counts
        for day in self._counts:
            if self.tree.exists(day):
                self.tree.item(day, text=self._row_text(day), tags=("imported",) if counts.get(day) == self._counts[day] else ())

    def _click(self, event) -> str | None:
        day = self.tree.identify_row(event.y)
        if day not in self._counts:
            return "break"
        self._focused = day
        self._toggle(day)
        self.tree.selection_set(day)
        self.tree.focus(day)
        self.tree.focus_set()
        self._notify()
        return "break"

    def _space(self, event) -> str:
        day = self.tree.focus()
        if day in self._counts:
            self._focused = day
            self._toggle(day)
            self._notify()
        return "break"

    def _focus_changed(self, event=None) -> None:
        selection = self.tree.selection()
        if selection and selection[0] in self._counts:
            day = selection[0]
            if day != self._focused:
                self._focused = day
                self._notify()

    def _toggle(self, day: str) -> None:
        if day in self._selected:
            self._selected.remove(day)
        else:
            self._selected.add(day)
        self.tree.item(day, text=self._row_text(day))
        self._update_all_label()

    def set_selected_dates(self, days: set[str]) -> None:
        """Set checked days and notify consumers."""
        self._selected = days.intersection(self._counts)
        for day in self._counts:
            if self.tree.exists(day):
                self.tree.item(day, text=self._row_text(day))
        self._focused = next(iter(sorted(self._selected, reverse=True)), None)
        if self._focused and self.tree.exists(self._focused):
            self.tree.selection_set(self._focused)
            self.tree.focus(self._focused)
        elif not self._focused:
            self.tree.selection_remove(*self.tree.selection())
            self.tree.focus("")
        self._update_all_label()
        self._notify()

    def toggle_all(self) -> None:
        self.set_selected_dates(
            set() if len(self._selected) == len(self._counts) else set(self._counts)
        )

    def _update_all_label(self) -> None:
        self.select_all_button.config(
            text="Auswahl aufheben"
            if self._selected and len(self._selected) == len(self._counts)
            else "Alle auswählen"
        )

    def _notify(self) -> None:
        if self.on_selection_changed:
            self.on_selection_changed(self.get_selected_dates())

    def get_selected_dates(self) -> set[str]:
        return self._selected.copy()

    def get_first_selected_date(self) -> str | None:
        """Return the focused day for preview, independent of checked days."""
        return self._focused

    def clear_selection(self) -> None:
        self.set_selected_dates(set())

    def is_empty(self) -> bool:
        return not self._counts

    def destroy(self) -> None:
        if self._populate_id:
            self.after_cancel(self._populate_id)
        super().destroy()
