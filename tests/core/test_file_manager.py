from datetime import datetime
import os
from pathlib import Path
from threading import Event

import pytest
from PIL import Image

from core.file_manager import DateSource, FileManager, ScanCancelledError


def test_recursive_scan_reuses_directory_stats(tmp_path, monkeypatch):
    nested = tmp_path / "nested"
    nested.mkdir()
    photo = nested / "photo.JPG"
    photo.write_bytes(b"photo")
    video = tmp_path / "video.mp4"
    video.touch()
    (tmp_path / "ignored.txt").touch()
    (tmp_path / "directory.jpg").mkdir()
    expected = FileManager().gather_files_by_date(tmp_path)
    stat = Path.stat

    def no_file_stat(path, *args, **kwargs):
        assert path not in (photo, video), "Should reuse DirEntry.stat()"
        return stat(path, *args, **kwargs)

    monkeypatch.setattr(Path, "stat", no_file_stat)
    progress = []
    result = FileManager().gather_files_by_date(
        tmp_path, on_progress=lambda *args: progress.append(args)
    )
    assert result == expected
    assert sorted(p for group in result.values() for p in group) == sorted([photo, video])
    assert progress == [(0, 2), (1, 2), (2, 2)]


def test_cancellation_during_discovery(tmp_path, monkeypatch):
    (tmp_path / "ignored.txt").touch()
    cancel = Event()
    scandir = os.scandir

    def cancelling_scandir(path):
        result = scandir(path)
        cancel.set()
        return result

    monkeypatch.setattr(os, "scandir", cancelling_scandir)
    with pytest.raises(ScanCancelledError):
        FileManager().gather_files_by_date(tmp_path, cancel_event=cancel)


def test_metadata_and_filesystem_fallback(tmp_path):
    photo = tmp_path / "capture.jpg"
    exif = Image.Exif()
    exif[306] = "2020:04:03 12:34:56"
    Image.new("RGB", (10, 10)).save(photo, exif=exif)
    video = tmp_path / "video.mov"
    video.touch()
    manager = FileManager()
    manager.set_date_source(DateSource.METADATA)
    result = manager.gather_files_by_date(tmp_path)
    assert photo in result["2020-04-03"]
    stat = video.stat()
    timestamp = stat.st_ctime if os.name == "nt" else stat.st_mtime
    assert video in result[datetime.fromtimestamp(timestamp).strftime("%Y-%m-%d")]
    assert manager.files_by_date == {}


def test_date_source_is_fixed_for_running_scan(tmp_path, monkeypatch):
    (tmp_path / "photo.jpg").touch()
    manager = FileManager()
    monkeypatch.setattr(
        manager, "_get_metadata_date",
        lambda *_: pytest.fail("A running scan must keep its initial date source"),
    )
    result = manager.gather_files_by_date(
        tmp_path, on_progress=lambda *_: manager.set_date_source(DateSource.METADATA)
    )
    assert sum(map(len, result.values())) == 1


@pytest.mark.parametrize("value, expected", [
    ("2024:02:29 10:11:12", "2024-02-29"),
    ("2023:02:29", None), ("invalid", None),
])
def test_exif_date_parsing(value, expected):
    assert FileManager()._parse_exif_datetime(value) == expected


def test_preview_limit_stops_reading_early():
    class Files(list):
        def __iter__(self):
            yield Path("photo.jpg")
            raise AssertionError("Should stop after the requested number of images")

    manager = FileManager()
    manager.files_by_date["date"] = Files()
    assert manager.get_image_files_for_preview("date", 1) == [Path("photo.jpg")]
    assert manager.get_image_files_for_preview("date", 0) == []


def test_missing_and_empty_folder(tmp_path):
    manager = FileManager()
    assert manager.gather_files_by_date(tmp_path / "missing") == {}
    assert manager.gather_files_by_date(tmp_path) == {}
