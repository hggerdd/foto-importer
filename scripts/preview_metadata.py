"""Render the metadata dashboard using an isolated sample collection.

Use --photo to supply a real preview image; otherwise a neutral test image is used.
The original photo is only read. No settings or files in a real collection change.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import sys
import tempfile
import time
import tkinter as tk
from pathlib import Path
from tkinter import ttk
from unittest.mock import patch

from PIL import Image, ImageDraw, ImageGrab

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import Settings
from core.collection_index import CollectionIndex
from ui.collection_controller import CollectionController
from ui.theme import configure_theme


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--photo", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="foto-metadata-preview-") as temporary:
        collection = Path(temporary) / "Sammlung"
        collection.mkdir()
        if args.photo:
            with Image.open(args.photo) as source:
                sample = source.convert("RGB")
                sample.thumbnail((1000, 650))
        else:
            # A neutral synthetic fixture, not a photograph from the collection.
            sample = Image.new("RGB", (900, 550), "#dce8ed")
            draw = ImageDraw.Draw(sample)
            for offset, color in enumerate(
                ("#cadce1", "#a7c6ce", "#729eac", "#3e788d")
            ):
                draw.rectangle((0, 170 + offset * 95, 900, 550), fill=color)
            draw.text(
                (35, 35), "VORSCHAUBILD / BEISPIELDATEN", fill="#365265", font_size=24
            )
        for number in range(30):
            path = collection / f"DSC_{4471 + number:04}.JPG"
            sample.save(path, quality=90)
            os.utime(path, (1700000000, 1700000000))
        raw = collection / "DSC_4471.NEF"
        raw.write_bytes(b"sample RAW placeholder")
        os.utime(raw, (1700000000, 1700000000))
        sidecar = collection / "DSC_4471.xmp"
        sidecar.write_text("sample sidecar")
        os.utime(sidecar, (1700000000, 1700000000))

        def metadata(paths, *_):
            results = []
            for path in paths:
                number = int(path.stem.split("_")[1]) - 4471
                camera = (
                    "Nikon Z6 II"
                    if number < 22
                    else "Nikon D750"
                    if number < 28
                    else "iPhone 13"
                )
                results.append(
                    {
                        "SourceFile": str(path),
                        "IFD0:Make": "NIKON" if number < 28 else "Apple",
                        "IFD0:Model": camera,
                        "ExifIFD:DateTimeOriginal": f"2025:07:{14 - number % 10:02} 18:42:07",
                        "ExifIFD:ISO": (100, 200, 400, 100, 100, 800, 1600, 3200)[
                            number % 8
                        ],
                        "ExifIFD:LensModel": (
                            "NIKKOR Z 24–70 mm f/4 S",
                            "NIKKOR 50 mm f/1.8",
                            "NIKKOR 70–200 mm",
                            "iPhone wide",
                        )[number % 4],
                        "ExifIFD:FNumber": 8,
                        "ExifIFD:ExposureTime": 0.004,
                        "ExifIFD:FocalLength": 35,
                        "File:ImageWidth": 6048,
                        "File:ImageHeight": 4024,
                        "Nikon:SerialNumber": "6038201",
                        "Nikon:ShutterCount": 12470 + number,
                        "XMP:Rating": 4,
                        "Composite:GPSLatitude": 46.636,
                        "Composite:GPSLongitude": 14.133,
                    }
                )
            return results

        with CollectionIndex.open(collection) as index:
            index.quick_scan()
            with patch("core.collection_index.read_metadata", metadata):
                index.enrich_metadata()
        root = tk.Tk()
        root.withdraw()
        configure_theme(root)
        controller = CollectionController(
            root, Settings(str(Path(temporary) / "settings.json")), lambda: collection
        )
        controller.open_window()
        controller.collection_path_var.set(r"C:\Bilder · Metadaten deiner Fotos")
        ttk.Label(controller.window, text="Beispieldaten", style="Muted.TLabel").place(
            relx=0.98, y=20, anchor="ne"
        )
        deadline = time.monotonic() + 20

        def wait_for_view():
            view = controller.metadata_view
            if (
                controller.busy
                or view.selected_detail is None
                or view.preview_image is None
            ):
                if time.monotonic() > deadline:
                    controller.close()
                    root.destroy()
                    raise RuntimeError("Metadata dashboard did not finish loading")
                root.after(100, wait_for_view)
                return
            root.after(250, capture)

        def capture():
            controller.window.update()
            controller.window.update_idletasks()
            get_parent = ctypes.windll.user32.GetParent
            get_parent.argtypes = [ctypes.c_void_p]
            get_parent.restype = ctypes.c_void_p
            handle = get_parent(controller.window.winfo_id())
            args.output.parent.mkdir(parents=True, exist_ok=True)
            ImageGrab.grab(window=handle).save(args.output)
            controller.close()
            root.destroy()

        root.after(300, wait_for_view)
        root.mainloop()


if __name__ == "__main__":
    main()
