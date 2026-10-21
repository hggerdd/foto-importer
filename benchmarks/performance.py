"""Compare the working tree with a Git revision using disposable test data.

Run: uv run python -m benchmarks.performance --baseline b0d08d7
Times are medians; scan/copy cases use small synthetic files on the local disk.
"""
from __future__ import annotations

import argparse
import gc
from pathlib import Path
from statistics import median
import subprocess
import sys
from tempfile import TemporaryDirectory
from time import perf_counter
import tkinter as tk
from types import ModuleType

from PIL import Image

from core.copy_worker import CopyJob
from core.file_manager import FileManager
from core.thumbnails import ThumbnailLoader
from ui.controllers import SourceScanController


def load_baseline(revision: str, path: str) -> ModuleType:
    source = subprocess.check_output(
        ["git", "show", f"{revision}:{path}"], text=True, encoding="utf-8"
    )
    module = ModuleType("baseline_" + path.replace("/", "_").replace(".", "_"))
    sys.modules[module.__name__] = module
    exec(compile(source, path, "exec"), module.__dict__)
    return module


def measure(action, repeats=3):
    values = []
    for _ in range(repeats):
        start = perf_counter()
        action()
        values.append(perf_counter() - start)
    return median(values)


def report(label, before, after):
    print(f"{label}: {before:.4f}s -> {after:.4f}s ({before / after:.2f}x)", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True)
    args = parser.parse_args()
    old_files = load_baseline(args.baseline, "core/file_manager.py")
    old_copy = load_baseline(args.baseline, "core/copy_worker.py")
    old_ui = load_baseline(args.baseline, "ui/controllers.py")

    with TemporaryDirectory(prefix="foto-importer-benchmark-") as temporary:
        folder = Path(temporary)
        source = folder / "source"
        source.mkdir()
        for group in range(30):
            nested = source / str(group)
            nested.mkdir()
            for index in range(200):
                (nested / f"{index}.jpg").touch()
        old, new = old_files.FileManager(), FileManager()
        assert old.gather_files_by_date(source) == new.gather_files_by_date(source)
        report(
            "Scan 6000 files (without UI)",
            measure(lambda: old.gather_files_by_date(source), 5),
            measure(lambda: new.gather_files_by_date(source), 5),
        )

        root = tk.Tk()
        root.withdraw()

        def scan_ui(controller_type, manager):
            controller = controller_type(root, manager)
            errors = []
            timeout = root.after(30000, lambda: (errors.append("timeout"), root.quit()))

            def start():
                controller.scan(
                    source, lambda: None, lambda *_: root.quit(),
                    lambda error: (errors.append(error), root.quit()),
                    on_progress=lambda *_: None,
                )

            root.after(0, start)
            root.mainloop()
            root.after_cancel(timeout)
            if hasattr(controller, "close"):
                controller.close()
            assert not errors, errors

        report(
            "Scan 6000 files (Tk progress delivery)",
            measure(lambda: scan_ui(old_ui.SourceScanController, old)),
            measure(lambda: scan_ui(SourceScanController, new)),
        )
        root.destroy()
        del root
        gc.collect()

        photos = folder / "photos"
        photos.mkdir()
        paths = [photos / f"{index}.jpg" for index in range(8)]
        image = Image.effect_noise((6000, 4000), 40).convert("RGB")
        for path in paths:
            image.save(path, quality=85)
        image.close()

        def old_thumbnails():
            for path in paths:
                with Image.open(path) as image:
                    image.thumbnail((150, 150), Image.Resampling.LANCZOS)
                    image.copy()

        def new_thumbnails():
            loader = ThumbnailLoader()
            for path in paths:
                loader.load(path, (150, 150))

        report("8 JPEG thumbnails (24 MP, empty app cache)",
               measure(old_thumbnails), measure(new_thumbnails))
        loader = ThumbnailLoader()
        for path in paths:
            loader.load(path, (150, 150))
        print("8 JPEG thumbnails (app cache hit): "
              f"{measure(lambda: [loader.load(p, (150, 150)) for p in paths]):.4f}s",
              flush=True)

        photo = source / "0" / "0.jpg"
        count = 0

        def copy(job_type):
            nonlocal count
            count += 1
            job = job_type([photo] * 400, folder / "copies", str(count))
            job.run()
            assert job.state.value == "completed", job.error
            assert len(list((folder / "copies" / str(count) / "jpg").iterdir())) == 400

        report("Copy 400 identical filenames (empty files)",
               measure(lambda: copy(old_copy.CopyJob)),
               measure(lambda: copy(CopyJob)))


if __name__ == "__main__":
    main()
