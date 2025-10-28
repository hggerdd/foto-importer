"""Camera File Organizer - Entry point."""
from __future__ import annotations

import tkinter as tk

from ui.main_window import MainWindow


def main() -> None:
    """Main entry point for the application."""
    root = tk.Tk()
    app = MainWindow(root)
    root.mainloop()


if __name__ == "__main__":
    main()
