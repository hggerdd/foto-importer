"""Render the import workspace with read-only sample data and isolated settings.

Run: uv run python scripts/preview_workspace.py --source D:/DCIM/103NZ6_2
Use --output docs/screenshots/import-workspace.png to capture and close it.
"""

from __future__ import annotations

import argparse
import ctypes
from pathlib import Path
import sys
import tempfile
from time import monotonic
import tkinter as tk

from PIL import ImageGrab

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from config.settings import Settings
import ui.main_window as main_window
import ui.dng_update_controller as updates


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--geometry", default="1586x1000+20+20")
    parser.add_argument(
        "--filesystem-dates",
        action="store_true",
        help="Skip EXIF reads when previewing a slow camera card.",
    )
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="foto-ui-preview-") as directory:
        settings = Settings(str(Path(directory) / "settings.json"))
        settings.settings.update(
            window_geometry=args.geometry,
            preview_count=12,
            date_source="filesystem" if args.filesystem_dates else "metadata",
            convert_to_dng=True,
        )
        main_window.Settings = lambda: settings
        updates.installed_version = lambda path: "18.7"
        updates.latest_version = lambda: "18.7"
        root = tk.Tk()
        app = main_window.MainWindow(root)
        app.folder_selector.source_label.config(text=str(args.source))
        app.folder_selector.target_label.config(text=r"C:\Bilder\2026")
        app.target_folder = Path(r"C:\Bilder\2026")
        app._on_source_selected(args.source)
        # The preview harness must never start an import.
        app.layout.execute_button.config(command=lambda: None)
        app.custom_name_entry.unbind("<Return>")
        capture_deadline = monotonic() + 90

        def select_when_ready():
            if not app.file_manager.files_by_date:
                root.after(100, select_when_ready)
                return
            day = (
                "2026-10-20"
                if "2026-10-20" in app.file_manager.files_by_date
                else max(app.file_manager.files_by_date)
            )
            app.date_list.set_selected_dates({day})
            app.custom_name_var.set(f"{day}_Seeausflug")
            app.progress_manager.add_progress_bar(
                "DNG: 2026-10-19_Bergtour", 21, lambda: False
            )
            app.progress_manager.update_progress("DNG: 2026-10-19_Bergtour", 14, 21)
            app.progress_manager.add_progress_bar("2026-10-18_Herbstwald", 42)
            app.progress_manager.finish_progress(
                "2026-10-18_Herbstwald", "completed: Import abgeschlossen"
            )
            if args.output:
                root.after(1500, capture)

        def capture():
            expected = len(app.preview_widget._paths)
            if not app._selection_counts or len(app.preview_widget._tiles) < expected:
                if monotonic() >= capture_deadline:
                    app._on_closing()
                    raise RuntimeError(
                        "Preview did not finish before screenshot timeout"
                    )
                root.after(100, capture)
                return
            root.update_idletasks()
            args.output.parent.mkdir(parents=True, exist_ok=True)
            # PrintWindow captures only this HWND, even behind another app.
            get_parent = ctypes.windll.user32.GetParent
            get_parent.argtypes = [ctypes.c_void_p]
            get_parent.restype = ctypes.c_void_p
            handle = get_parent(root.winfo_id())
            ImageGrab.grab(window=handle).save(args.output)
            app._on_closing()

        root.after(100, select_when_ready)
        root.mainloop()


if __name__ == "__main__":
    main()
