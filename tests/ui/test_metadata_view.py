"""Exercise real browser controls, previews, and stale worker result handling."""

import threading
import time
import tkinter as tk

import pytest
from PIL import Image

from core.collection_index import CollectionIndex
from ui.metadata_view import MetadataView, display_date, exposure_text


def pump(root, condition):
    deadline = time.monotonic() + 8
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition()


@pytest.fixture
def browser(tmp_path, monkeypatch, tk_root):
    for name in ("one.jpg", "two.jpg"):
        Image.new("RGB", (160, 100), "#4a9bbb").save(tmp_path / name)

    def metadata(paths, *_):
        return [
            {
                "SourceFile": str(path),
                "IFD0:Model": "Nikon Z6 II" if path.name == "one.jpg" else "Nikon D750",
                "ExifIFD:DateTimeOriginal": "2025:07:14 18:42:07"
                if path.name == "one.jpg"
                else "2024:01:01 12:00:00",
                "ExifIFD:ISO": 100,
                "ExifIFD:FNumber": 8,
                "ExifIFD:FocalLength": 35,
                "ExifIFD:ExposureTime": 0.004,
            }
            for path in paths
        ]

    monkeypatch.setattr("core.collection_index.read_metadata", metadata)
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        index.enrich_metadata()
    root = tk.Toplevel(tk_root)
    root.withdraw()
    view = MetadataView(root, lambda: tmp_path, lambda: None)
    view.pack(fill="both", expand=True)
    pump(root, lambda: view.selected_detail is not None)
    yield root, view, tmp_path
    view.close()
    root.destroy()


def test_real_filters_preview_and_detail(browser):
    root, view, _ = browser
    assert view.total == 2
    assert view.summary_vars[1].get() == "100 %"
    assert view.preview_image is not None
    assert "1/250 s" in view.exposure_var.get()
    assert "ISO 100" in view.exposure_var.get()
    assert view.detail_vars["model"].get() == "Nikon Z6 II"
    view.year.set("2024")
    view.year_combo.event_generate("<<ComboboxSelected>>")
    pump(
        root,
        lambda: (
            view.total == 1
            and view.selected_detail is not None
            and view.name_var.get() == "two.jpg"
        ),
    )
    assert view.camera_chart.values == [("Nikon D750", 1)]
    assert sum(value for _, value in view.iso_chart.values) == 1
    view.search.set("absent")
    pump(root, lambda: view.total == 0)
    assert not view.tree.get_children()
    assert view.selected_detail is None
    assert view.preview_image is None
    assert str(view.reveal_button.cget("state")) == "disabled"


def test_sort_and_reveal_selected_file(browser, monkeypatch):
    root, view, collection = browser
    view._sort("name")
    pump(root, lambda: view.selected_detail is not None)
    assert list(view.rows.values())[0]["name"] == "one.jpg"
    calls = []
    monkeypatch.setattr(
        "ui.metadata_view.subprocess.Popen", lambda args: calls.append(args)
    )
    view.reveal_button.invoke()
    assert str(collection / "one.jpg") in calls[0]
    view.extra_button.invoke()
    assert view.extra.winfo_manager() == "pack"
    assert "Seriennummer" in view.extra_var.get()
    view.extra_button.invoke()
    assert not view.extra.winfo_manager()


def test_slow_old_query_cannot_replace_new_filter(browser, monkeypatch):
    root, view, _ = browser
    from ui import metadata_view

    real = metadata_view.load_metadata_page
    started, release = threading.Event(), threading.Event()

    def delayed(path, filters):
        if filters.search == "one":
            started.set()
            assert release.wait(5)
        return real(path, filters)

    monkeypatch.setattr(metadata_view, "load_metadata_page", delayed)
    view.search.set("one")
    view.reload()
    pump(root, started.is_set)
    view.search.set("two")
    view.reload()
    heartbeat = []
    root.after(0, lambda: heartbeat.append(True))
    root.update()
    assert heartbeat
    release.set()
    pump(
        root,
        lambda: view.selected_detail is not None and view.name_var.get() == "two.jpg",
    )
    assert view.total == 1


def test_old_detail_cannot_reappear_after_filter_change(browser, monkeypatch):
    root, view, _ = browser
    from ui import metadata_view

    real = metadata_view.load_metadata_detail
    started, release = threading.Event(), threading.Event()

    def delayed(*args):
        started.set()
        assert release.wait(5)
        return real(*args)

    monkeypatch.setattr(metadata_view, "load_metadata_detail", delayed)
    view._selected()
    pump(root, started.is_set)
    view.search.set("nothing")
    view.reload()
    pump(root, lambda: view.total == 0)
    release.set()
    pump(root, lambda: view._detail_future.done())
    root.update()
    assert view.selected_detail is None and view.preview_image is None


def test_close_during_query_drops_callbacks(browser, monkeypatch):
    root, view, _ = browser
    started, release = threading.Event(), threading.Event()

    def delayed(*_):
        started.set()
        assert release.wait(5)
        raise OSError("late failure")

    monkeypatch.setattr("ui.metadata_view.load_metadata_page", delayed)
    view.reload()
    pump(root, started.is_set)
    view.close()
    release.set()
    pump(root, view._future.done)
    assert view._closed


def test_missing_index_does_not_create_files(tmp_path, tk_root):
    view = MetadataView(tk_root, lambda: tmp_path, lambda: None)
    try:
        pump(tk_root, lambda: "Noch kein Index" in view.footer.get())
        assert not list(tmp_path.iterdir())
        assert view.summary_vars[0].get() == "0"
    finally:
        view.destroy()


def test_formatting_handles_absent_and_original_exif_values():
    assert display_date("2025:07:14 18:42:07") == "14.07.2025 18:42:07"
    assert display_date(None) == "–"
    assert display_date("invalid") == "invalid"
    assert exposure_text("0.004") == "1/250 s"
    assert exposure_text("1/125") == "1/125 s"
    assert exposure_text(2) == "2 s"
