"""Exercise responsiveness and selection behavior using real Tk controls."""

from datetime import date, timedelta
from pathlib import Path
from threading import Event, get_ident
import time
import tkinter as tk

import pytest
from PIL import Image

from config.settings import Settings
from ui.date_list_widget import DateListWidget
from ui.main_window import MainWindow
from ui.progress_manager import ProgressManager


def pump(root, condition):
    deadline = time.monotonic() + 5
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition()


@pytest.fixture
def workspace(tmp_path, monkeypatch, tk_root):
    settings = Settings(str(tmp_path / "settings.json"))
    monkeypatch.setattr("ui.main_window.Settings", lambda: settings)
    root = tk.Toplevel(tk_root)
    root.withdraw()
    app = MainWindow(root)
    app.target_folder = tmp_path / "target"
    photo = tmp_path / "photo.jpg"
    Image.new("RGB", (30, 20)).save(photo)
    app.file_manager.files_by_date = {
        "2026-10-20": [photo],
        "2026-10-19": [tmp_path / "sample.nef", tmp_path / "clip.mov"],
    }
    app._populate_date_list()
    yield app
    app._on_closing()


def test_deselect_last_day_disables_import_and_clears_summary(workspace):
    app = workspace
    app.date_list.set_selected_dates({"2026-10-20"})
    pump(app.root, lambda: app._selection_counts == {"jpg": 1})
    assert app.execute_button.cget("text") == "1 Datei importieren →"
    assert str(app.execute_button.cget("state")) == "normal"
    app.date_list.clear_selection()
    pump(app.root, lambda: not app._selection_counts)
    assert str(app.execute_button.cget("state")) == "disabled"
    assert "Kein Tag" in app.layout.selection_summary.cget("text")


def test_clicking_days_toggles_checkboxes_without_modifier_keys(workspace):
    app = workspace
    app.root.deiconify()
    app.root.update()
    tree = app.date_list.tree

    def click(day):
        x, y, width, height = tree.bbox(day)
        tree.event_generate("<Button-1>", x=x + 20, y=y + height // 2)
        app.root.update()

    click("2026-10-20")
    click("2026-10-19")
    assert app.date_list.get_selected_dates() == {"2026-10-20", "2026-10-19"}
    click("2026-10-20")
    assert app.date_list.get_selected_dates() == {"2026-10-19"}
    assert app.date_list.get_first_selected_date() == "2026-10-20"
    click("2026-10-19")
    assert not app.date_list.get_selected_dates()
    assert str(app.execute_button.cget("state")) == "disabled"


def test_summary_keeps_ui_responsive_and_discards_old_selection(workspace, monkeypatch):
    from core.import_plan import summarize_selection

    app = workspace
    owner = get_ident()
    started, release = Event(), Event()

    def slow(*args):
        assert get_ident() != owner
        if args[2] == "2026-10-20":
            started.set()
            assert release.wait(4)
        return summarize_selection(*args)

    monkeypatch.setattr("ui.selection_controller.summarize_selection", slow)
    try:
        app.date_list.set_selected_dates({"2026-10-20"})
        assert started.wait(1)
        heartbeat = []
        app.root.after(0, lambda: heartbeat.append(True))
        app.root.update()
        assert heartbeat
        app.date_list.set_selected_dates({"2026-10-19"})
        release.set()
        pump(app.root, lambda: app._selection_counts == {"nef": 1, "mov": 1})
        assert "19. Oktober" in app.layout.preview_title.cget("text")
        assert not app.preview_widget.image_references
        labels = app.preview_widget.scrollable_frame.winfo_children()
        assert "RAW-Dateien und Videos" in labels[0].cget("text")
    finally:
        release.set()


def test_slow_destination_probe_keeps_ui_alive_and_captures_target(
    workspace, monkeypatch, tmp_path
):
    app = workspace
    app.date_list.set_selected_dates({"2026-10-20"})
    app.custom_name_var.set("album")
    destination = app.target_folder / "album"
    owner = get_ident()
    original = Path.exists
    started, release = Event(), Event()

    def slow_exists(path):
        if path == destination:
            assert get_ident() != owner
            started.set()
            assert release.wait(4)
        return original(path)

    monkeypatch.setattr(Path, "exists", slow_exists)
    try:
        app._execute_copy()
        assert started.wait(1)
        assert app._preflight_pending
        heartbeat = []
        app.root.after(0, lambda: heartbeat.append(True))
        app.root.update()
        assert heartbeat
        app.target_folder = tmp_path / "other"
        app.custom_name_var.set("changed")
        release.set()
        pump(
            app.root,
            lambda: (
                (destination / "jpg" / "photo.jpg").is_file()
                and not app._pending_groups
            ),
        )
        assert not (tmp_path / "other" / "changed").exists()
    finally:
        release.set()


def test_source_change_invalidates_pending_preflight(workspace, monkeypatch):
    app = workspace
    app.date_list.set_selected_dates({"2026-10-20"})
    app.custom_name_var.set("album")
    callbacks = []
    monkeypatch.setattr(
        app.import_preflight, "check", lambda *args: callbacks.append(args[-1])
    )
    app._execute_copy()
    app._on_scan_started()
    callbacks[0](False, None, [Path("photo.jpg")])
    assert not app._pending_groups
    assert app.copy_worker.get_active_job_count() == 0


def test_cancelled_scan_does_not_offer_files_from_another_source(workspace):
    app = workspace
    app.file_manager.source_folder = Path("camera-a")
    app.source_folder = Path("camera-b")
    app._on_scan_cancelled()
    assert app.date_list.is_empty()


def test_failed_old_import_does_not_restore_days_into_another_source(workspace):
    app = workspace
    app._pending_groups["album"] = {"2025-01-01": [Path("old.jpg")]}
    app._pending_sources["album"] = Path("camera-a")
    app.source_folder = Path("camera-b")
    app._restore_pending_groups("album")
    assert "2025-01-01" not in app.file_manager.files_by_date
    assert not app._pending_groups


def test_empty_gallery_zoom_does_not_resurrect_previous_images(workspace):
    app = workspace
    app.date_list.set_selected_dates({"2026-10-20"})
    pump(app.root, lambda: len(app.preview_widget.image_references) == 1)
    app.preview_widget.clear_previews()
    app.preview_widget.set_thumbnail_size(200)
    app.root.update()
    assert not app.preview_widget._paths
    assert not app.preview_widget.image_references


def test_large_date_list_population_yields_and_can_be_replaced(tk_root):
    widget = DateListWidget(tk_root)
    start = date(2020, 1, 1)
    groups = {(start + timedelta(days=i)).isoformat(): 1 for i in range(2000)}
    try:
        widget.populate(groups)
        assert widget._populate_id is not None
        heartbeat = []
        tk_root.after(0, lambda: heartbeat.append(True))
        pump(tk_root, lambda: bool(heartbeat))
        widget.populate({"2026-10-20": 42})
        tk_root.update()
        assert widget.tree.get_children() == ("month:2026-10", "2026-10-20")
        widget.set_selected_dates({"2026-10-20"})
        assert widget.get_selected_dates() == {"2026-10-20"}
        widget.toggle_all()
        assert not widget.get_selected_dates()
    finally:
        widget.destroy()


def test_job_rows_are_bounded_and_history_retains_results(tk_root):
    frame = tk.Frame(tk_root)
    progress = ProgressManager(frame)
    try:
        for i in range(12):
            progress.add_progress_bar(str(i), 10)
            progress.finish_progress(str(i), "completed")
        assert len(progress.progress_bars) == 2
        assert len(progress._history) == 12
        progress.add_progress_bar("active", 10, lambda: True)
        assert len(progress.progress_bars) == 3
        assert not progress.progress_bars["active"].terminal
        progress.show_history()
        assert progress._history_window.winfo_exists()
    finally:
        if progress._history_window:
            progress._history_window.destroy()
        frame.destroy()


@pytest.mark.parametrize("geometry", ["1080x720", "1586x1000"])
def test_primary_action_stays_visible_and_settings_are_modeless(workspace, geometry):
    app = workspace
    app.root.geometry(geometry)
    app.root.deiconify()
    app.root.update()
    button = app.execute_button
    y = button.winfo_rooty() - app.root.winfo_rooty()
    assert 0 <= y < app.root.winfo_height() - button.winfo_height()
    app.layout.open_settings()
    app.root.update()
    assert app.root.grab_current() is None
    assert app.layout.settings_window.state() == "normal"
