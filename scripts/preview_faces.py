"""Capture the real face workspace with isolated, synthetic review fixtures."""

from __future__ import annotations

import argparse
import ctypes
import json
import sys
import tempfile
import time
import tkinter as tk
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageGrab

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import Settings
from core.collection_index import CollectionIndex
from core.face_service import FaceService
from ui.collection_controller import CollectionController
from ui.theme import configure_theme


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="foto-face-preview-") as temporary:
        collection = Path(temporary) / "Sammlung"
        collection.mkdir()
        for i in range(12):
            image = Image.new(
                "RGB", (300, 300), ("#cadde8", "#e7d7c1", "#d3decb")[i // 4]
            )
            draw = ImageDraw.Draw(image)
            draw.ellipse(
                (65, 35, 235, 225), fill=("#d5ac87", "#be906b", "#edc8a1")[i // 4]
            )
            draw.arc((65, 20, 235, 170), 180, 355, fill="#58483e", width=32)
            draw.ellipse((110, 110, 121, 122), fill="#3e4448")
            draw.ellipse((178, 110, 189, 122), fill="#3e4448")
            draw.arc((115, 125, 185, 185), 5, 175, fill="#7a5145", width=4)
            draw.ellipse(
                (70, 225, 230, 380), fill=("#56899f", "#bc7b5f", "#7a9372")[i // 4]
            )
            image.save(collection / f"Aufnahme_{i + 1:03}.jpg")
        with CollectionIndex.open(collection) as index:
            index.quick_scan()
            service = FaceService(index)
            service.create_person({"name": "Anna", "birth_year": "1988"})
            for i, source in enumerate(
                index.db.execute("SELECT * FROM files ORDER BY id").fetchall()
            ):
                vector = np.zeros(128, dtype="<f4")
                vector[i // 4] = 1
                with index.db:
                    face = index.db.execute(
                        """INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,
                        bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,captured_at)
                        VALUES(?,?,?,?,.2,.1,.8,.75,?,.99,.92,?)""",
                        (
                            source["photo_id"],
                            source["id"],
                            source["relpath"],
                            service.detector_key,
                            json.dumps(
                                [
                                    [0.4, 0.4],
                                    [0.6, 0.4],
                                    [0.5, 0.5],
                                    [0.4, 0.6],
                                    [0.6, 0.6],
                                ]
                            ),
                            f"{2020 + i % 4}:07:14 18:00:00",
                        ),
                    ).lastrowid
                    index.db.execute(
                        "INSERT INTO face_embeddings VALUES(?,?,?,?)",
                        (face, service.embedder_key, 128, vector.tobytes()),
                    )
        root = tk.Tk()
        root.withdraw()
        configure_theme(root)
        controller = CollectionController(
            root, Settings(str(Path(temporary) / "settings.json")), lambda: collection
        )
        controller.open_window()
        controller.collection_path_var.set("Beispieldaten · gezeichnete Testgesichter")
        controller.notebook.select(controller.face_view)
        deadline = time.monotonic() + 15

        def capture():
            if controller.face_controller.busy or not controller.face_view.snapshot:
                if time.monotonic() > deadline:
                    controller.close()
                    root.destroy()
                    raise RuntimeError("Face workspace did not finish loading")
                root.after(100, capture)
                return
            root.after(400, save)

        def save():
            controller.window.update()
            get_parent = ctypes.windll.user32.GetParent
            get_parent.argtypes = [ctypes.c_void_p]
            get_parent.restype = ctypes.c_void_p
            handle = get_parent(controller.window.winfo_id())
            args.output.parent.mkdir(parents=True, exist_ok=True)
            ImageGrab.grab(window=handle).save(args.output)
            controller.close()
            root.destroy()

        root.after(700, capture)
        root.mainloop()


if __name__ == "__main__":
    main()
