from threading import Event, get_ident
import time

import pytest
from PIL import Image

from ui.preview_widget import PreviewWidget
from config.settings import PREVIEW_PREFETCH_ROWS
from core.task_scheduler import TaskScheduler


@pytest.fixture
def root(tk_root):
    return tk_root


def pump_until(root, condition):
    deadline = time.monotonic() + 5
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition(), "UI callback did not arrive"


def test_loading_does_not_block_ui_and_discards_stale_results(
    root, tmp_path, monkeypatch
):
    widget = PreviewWidget(root)
    owner = get_ident()
    started, release = Event(), Event()

    def slow_load(path, size):
        assert get_ident() != owner
        if path.name == "old.jpg":
            started.set()
            assert release.wait(timeout=5)
        return Image.new("RGB", (10, 10))

    monkeypatch.setattr(widget._loader, "load", slow_load)
    try:
        widget.load_previews([tmp_path / "old.jpg"])
        assert started.wait(timeout=5)
        heartbeat = []
        root.after(0, lambda: heartbeat.append(True))
        root.update()
        assert heartbeat  # Decoder is still blocked, while Tk remains responsive.
        widget.load_previews([tmp_path / "new.jpg"])
        release.set()
        pump_until(root, lambda: len(widget.image_references) == 1)
        assert widget.canvas.itemcget(widget._tiles[0].caption, "text") == "new.jpg"
        widget.clear_previews()
        assert not widget.image_references
    finally:
        release.set()
        widget.destroy()


def test_corrupt_image_does_not_stop_remaining_previews(root, tmp_path):
    bad, good = tmp_path / "bad.jpg", tmp_path / "good.jpg"
    bad.write_bytes(b"invalid")
    Image.new("RGB", (20, 20)).save(good)
    widget = PreviewWidget(root)
    widget.load_previews([bad, good])
    pump_until(root, lambda: len(widget._ready_images) == 2)
    assert widget._ready_images[0] is None
    assert widget._ready_images[1] is not None
    widget.destroy()


@pytest.fixture
def gallery(tk_root):
    import tkinter as tk

    window = tk.Toplevel(tk_root)
    window.geometry("640x400")
    scheduler = TaskScheduler(settle_seconds=0)
    widget = PreviewWidget(window, scheduler=scheduler)
    widget.pack(fill="both", expand=True)
    window.update()
    yield window, widget, scheduler
    window.destroy()


def test_ten_thousand_images_reserve_space_but_load_only_viewport(
    gallery, tmp_path, monkeypatch
):
    window, widget, scheduler = gallery
    loaded = []

    def decode(path, size):
        loaded.append(int(path.stem))
        return Image.new("RGB", size)

    monkeypatch.setattr(widget._loader, "load", decode)
    paths = [tmp_path / f"{i:05d}.jpg" for i in range(10000)]
    widget.load_previews(paths)
    assert not widget.image_references
    assert len(widget.canvas.find_all()) == len(widget._visible_indices) * 3
    assert all(
        widget.canvas.itemcget(tile.placeholder, "fill") == "white"
        for tile in widget._tiles.values()
    )
    pump_until(
        window,
        lambda: (
            len(widget._ready_images) == len(widget._wanted_indices)
            and scheduler._foreground == 0
        ),
    )
    assert set(loaded) == widget._wanted_indices
    assert (
        len(loaded)
        <= len(widget._visible_indices) + 2 * PREVIEW_PREFETCH_ROWS * widget._columns
    )
    assert len(widget.image_references) == len(widget._visible_indices)
    assert len(widget.canvas.winfo_children()) == 0  # No Tk widget per file.
    assert float(widget.canvas.cget("scrollregion").split()[-1]) > 500000

    widget.canvas.yview_moveto(1)
    widget._refresh_viewport()
    pump_until(window, lambda: widget.viewport_ready and scheduler._foreground == 0)
    assert 9999 in widget.image_references
    assert 0 not in widget.image_references
    assert set(widget.image_references) == widget._visible_indices
    assert len(widget.canvas.find_all()) == len(widget._visible_indices) * 3
    assert (
        len(widget._ready_images)
        <= len(widget._visible_indices) + 2 * PREVIEW_PREFETCH_ROWS * widget._columns
    )


def test_fast_scroll_skips_intermediate_pages_and_keeps_tk_responsive(
    gallery, tmp_path, monkeypatch
):
    window, widget, scheduler = gallery
    started, release = Event(), Event()
    loaded = []

    def decode(path, size):
        index = int(path.stem)
        loaded.append(index)
        if index == 0:
            started.set()
            assert release.wait(3)
        return Image.new("RGB", size)

    monkeypatch.setattr(widget._loader, "load", decode)
    try:
        widget.load_previews([tmp_path / f"{i:05d}.jpg" for i in range(10000)])
        assert started.wait(1)
        for fraction in (0.2, 0.4, 0.6, 0.8, 1):
            widget.canvas.yview_moveto(fraction)
            widget._refresh_viewport()
        final = widget._wanted_indices.copy()
        heartbeat = []
        window.after(0, lambda: heartbeat.append(True))
        window.update()
        assert heartbeat
        release.set()
        pump_until(window, lambda: widget.viewport_ready and scheduler._foreground == 0)
        assert set(loaded) <= {0} | final
        assert set(widget.image_references) == widget._visible_indices
        assert 9999 in widget.image_references
    finally:
        release.set()


def test_zoom_discards_old_sizes_and_resize_preserves_the_visible_day(
    gallery, tmp_path, monkeypatch
):
    window, widget, scheduler = gallery
    started, release = Event(), Event()

    def decode(path, size):
        if size == (210, 140):
            started.set()
            assert release.wait(3)
        return Image.new("RGB", size)

    monkeypatch.setattr(widget._loader, "load", decode)
    try:
        widget.load_previews([tmp_path / f"{i:05d}.jpg" for i in range(1000)])
        assert started.wait(1)
        widget.set_thumbnail_size(300)
        release.set()
        pump_until(window, lambda: widget.viewport_ready and scheduler._foreground == 0)
        assert all(photo.width() == 300 for photo in widget.image_references.values())
        widget.canvas.yview_moveto(0.5)
        widget._refresh_viewport()
        pump_until(window, lambda: widget.viewport_ready and scheduler._foreground == 0)
        anchor = min(widget._visible_indices)
        window.geometry("380x400")
        window.update()
        widget._refresh_viewport()
        pump_until(window, lambda: widget.viewport_ready and scheduler._foreground == 0)
        assert widget._columns == 1
        assert anchor in widget._visible_indices
        assert set(widget.image_references) == widget._visible_indices
    finally:
        release.set()
