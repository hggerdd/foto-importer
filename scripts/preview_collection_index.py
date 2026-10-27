"""Render the collection index window with a safe generated sample collection."""

import argparse
import ctypes
import sys
import tempfile
import tkinter as tk
from pathlib import Path

from PIL import ImageGrab

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import Settings
from core.collection_index import CollectionIndex
from ui.collection_controller import CollectionController
from ui.theme import configure_theme


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="foto-index-preview-") as temporary:
        collection = Path(temporary) / "Sammlung"
        collection.mkdir()
        album = collection / "2025-07-14"
        album.mkdir()
        for number in range(8):
            (album / f"DSC_{number:04}.NEF").write_bytes(f"sample-{number}".encode())
        with CollectionIndex.open(collection) as index:
            index.quick_scan()
        album.rename(collection / "2025-07-14 Wörthersee")
        (collection / "Neu.jpg").write_bytes(b"new")
        root = tk.Tk()
        root.withdraw()
        configure_theme(root)
        controller = CollectionController(
            root, Settings(str(Path(temporary) / "settings.json")), lambda: collection
        )
        controller.open_window()

        def scan():
            if controller.busy:
                root.after(50, scan)
                return
            controller.run("quick_scan")
            root.after(100, capture)

        def capture():
            if controller.busy:
                root.after(50, capture)
                return
            controller.window.update_idletasks()
            get_parent = ctypes.windll.user32.GetParent
            get_parent.argtypes = [ctypes.c_void_p]
            get_parent.restype = ctypes.c_void_p
            handle = get_parent(controller.window.winfo_id())
            args.output.parent.mkdir(parents=True, exist_ok=True)
            ImageGrab.grab(window=handle).save(args.output)
            controller.close()
            root.destroy()

        root.after(100, scan)
        root.mainloop()


if __name__ == "__main__":
    main()
