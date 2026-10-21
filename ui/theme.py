"""Shared colours and styles for the import workspace."""

import tkinter as tk
from tkinter import ttk

CANVAS = "#f4f5f7"
SURFACE = "#ffffff"
TEXT = "#20252c"
MUTED = "#68717e"
ACCENT = "#0866ed"


def configure_theme(root: tk.Misc) -> None:
    """Use consistent desktop controls without external theme dependencies."""
    style = ttk.Style(root)
    style.theme_use("clam")
    root.configure(background=CANVAS)
    style.configure(".", font=("Segoe UI", 11), background=SURFACE, foreground=TEXT)
    style.configure("TFrame", background=SURFACE)
    style.configure("Canvas.TFrame", background=CANVAS)
    style.configure("TLabel", background=SURFACE)
    style.configure("Muted.TLabel", foreground=MUTED)
    style.configure("Title.TLabel", font=("Segoe UI", 21, "bold"))
    style.configure("Heading.TLabel", font=("Segoe UI", 13, "bold"))
    style.configure("Small.TLabel", font=("Segoe UI", 10), foreground=MUTED)
    style.configure(
        "TButton",
        padding=(12, 7),
        background="#f8f9fb",
        bordercolor="#dce0e5",
        focusthickness=1,
    )
    style.map("TButton", background=[("active", "#e9eef6")])
    style.configure(
        "Link.TButton",
        foreground=ACCENT,
        background=SURFACE,
        borderwidth=0,
        padding=(4, 4),
    )
    style.configure(
        "Primary.TButton",
        background=ACCENT,
        foreground="white",
        font=("Segoe UI", 11, "bold"),
        padding=(12, 12),
    )
    style.map(
        "Primary.TButton",
        background=[("disabled", "#e1e6ed"), ("active", "#0053cc")],
        foreground=[("disabled", "#7c8797"), ("!disabled", "white")],
    )
    style.configure("TEntry", padding=8, fieldbackground=SURFACE)
    style.configure("TCombobox", padding=5)
    style.configure(
        "Treeview",
        rowheight=44,
        borderwidth=0,
        fieldbackground=SURFACE,
        background=SURFACE,
        bordercolor=SURFACE,
        lightcolor=SURFACE,
        darkcolor=SURFACE,
        relief="flat",
    )
    style.map(
        "Treeview",
        background=[("selected", "#e3efff")],
        foreground=[("selected", TEXT)],
    )
    style.configure(
        "Horizontal.TProgressbar",
        background=ACCENT,
        troughcolor="#e6eaf0",
        borderwidth=0,
        thickness=6,
    )
    style.configure("TSeparator", background="#e5e8ed")
    style.configure(
        "TScrollbar",
        background="#d8dfe9",
        troughcolor="#f7f8fa",
        borderwidth=0,
        bordercolor=SURFACE,
        arrowsize=12,
    )
    style.configure(
        "Horizontal.TScale", background=SURFACE, troughcolor="#dde4ed", borderwidth=0
    )
