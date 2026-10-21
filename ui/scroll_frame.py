"""A local-wheel scrolling surface for compact inspector windows."""

import tkinter as tk
from tkinter import ttk


class ScrollFrame(ttk.Frame):
    """Keep import controls reachable on smaller displays."""

    def __init__(self, parent: tk.Misc) -> None:
        super().__init__(parent)
        self.canvas = tk.Canvas(
            self, highlightthickness=0, bg="white", width=310, height=1
        )
        scrollbar = ttk.Scrollbar(self, command=self.canvas.yview)
        self.canvas.configure(yscrollcommand=scrollbar.set)
        scrollbar.pack(side="right", fill="y")
        self.canvas.pack(side="left", fill="both", expand=True)
        self.content = ttk.Frame(self.canvas)
        self._window = self.canvas.create_window(
            (0, 0), window=self.content, anchor="nw"
        )
        self.canvas.bind(
            "<Configure>",
            lambda e: self.canvas.itemconfigure(self._window, width=e.width),
        )
        self.content.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all")),
        )

    def bind_wheel(self) -> None:
        """Attach only to children of this surface, leaving other panes alone."""

        def wheel(event):
            self.canvas.yview_scroll(-int(event.delta / 120), "units")
            return "break"

        def visit(widget):
            widget.bind("<MouseWheel>", wheel)
            for child in widget.winfo_children():
                visit(child)

        visit(self)
