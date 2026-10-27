"""Query real collection caches with paired variants and partial EXIF data."""

import io
import os

import pytest
from PIL import Image

from core.collection_index import CollectionIndex
from core.metadata_browser import (
    MetadataFilter,
    load_metadata_detail,
    load_metadata_page,
    _raw_preview,
)


@pytest.fixture
def metadata_collection(tmp_path, monkeypatch):
    root = tmp_path / "collection"
    root.mkdir()
    specs = {
        "DSC_0001": ("Nikon Z6 II", "2025:07:14 18:42:07", 100, "NIKKOR 24–70"),
        "DSC_0002": ("Nikon D750", "2024:01:01 09:00:00", 1600, "NIKKOR 50"),
        "Bär_0003": ("Nikon Z6 II", "2025:06:01 13:00:00", 6400, "NIKKOR 24–70"),
    }
    for name in specs:
        path = root / f"{name}.jpg"
        Image.new("RGB", (120, 80), "#4a9bbb").save(path)
        os.utime(path, (1700000000, 1700000000))
    for ext in ("nef", "xmp"):
        path = root / f"DSC_0001.{ext}"
        path.write_bytes(b"test")
        os.utime(path, (1700000000, 1700000000))
    Image.new("RGB", (120, 80)).save(root / "pending.jpg")

    def metadata(paths, *_):
        results = []
        for path in paths:
            if path.stem == "pending":
                results.append({"SourceFile": str(path), "Error": "Unsupported"})
                continue
            camera, date, iso, lens = specs[path.stem]
            results.append(
                {
                    "SourceFile": str(path),
                    "IFD0:Model": camera,
                    "ExifIFD:DateTimeOriginal": date,
                    "ExifIFD:ISO": iso,
                    "ExifIFD:LensModel": lens,
                    "ExifIFD:ExposureTime": 0.004,
                    "ExifIFD:FNumber": 8,
                    "ExifIFD:FocalLength": 35,
                    "File:ImageWidth": 6048,
                    "File:ImageHeight": 4024,
                    "XMP:Rating": 4 if path.suffix == ".xmp" else 2,
                }
            )
        return results

    monkeypatch.setattr("core.collection_index.read_metadata", metadata)
    with CollectionIndex.open(root) as index:
        index.quick_scan()
        index.enrich_metadata()
    return root


def test_summaries_count_photos_not_variants(metadata_collection):
    result = load_metadata_page(metadata_collection, MetadataFilter())
    assert result["summary"] == dict(total=4, metadata=3, cameras=2, lenses=2)
    assert result["camera_counts"] == [
        ("Nikon Z6 II", 2),
        ("Nikon D750", 1),
        ("Unbekannt", 1),
    ]
    assert result["iso_counts"] == [1, 0, 0, 0, 1, 1]
    assert result["years"] == ["2025", "2024"]
    assert result["last_metadata"]
    assert not (metadata_collection / ".myfotomanager.lock").exists()


def test_filters_unicode_literal_search_and_sort(metadata_collection):
    root = metadata_collection
    assert (
        load_metadata_page(root, MetadataFilter(search="BÄR"))["summary"]["total"] == 1
    )
    assert load_metadata_page(root, MetadataFilter(search="%"))["summary"]["total"] == 0
    assert (
        load_metadata_page(
            root, MetadataFilter(search="24–70", year="2025", camera="Nikon Z6 II")
        )["summary"]["total"]
        == 2
    )
    assert (
        load_metadata_page(root, MetadataFilter(year="2024"))["rows"][0]["name"]
        == "DSC_0002.jpg"
    )
    rows = load_metadata_page(root, MetadataFilter(sort="iso", descending=True))["rows"]
    assert rows[0]["iso"] == 6400
    assert (
        len(load_metadata_page(root, MetadataFilter(sort="DROP TABLE files"))["rows"])
        == 4
    )


def test_page_boundaries_and_stable_order(metadata_collection, monkeypatch):
    monkeypatch.setattr("core.metadata_browser.METADATA_PAGE_SIZE", 2)
    first = load_metadata_page(metadata_collection, MetadataFilter(page=0))
    last = load_metadata_page(metadata_collection, MetadataFilter(page=99))
    assert first["page"] == 0 and last["page"] == 1
    assert len(first["rows"]) == len(last["rows"]) == 2
    assert {row["photo_id"] for row in first["rows"]}.isdisjoint(
        row["photo_id"] for row in last["rows"]
    )


def test_detail_uses_jpeg_preview_of_raw_and_xmp_rating(metadata_collection):
    page = load_metadata_page(metadata_collection, MetadataFilter(search="DSC_0001"))
    photo_id = page["rows"][0]["photo_id"]
    detail = load_metadata_detail(metadata_collection, photo_id)
    assert detail["name"] == "DSC_0001.nef"
    assert detail["image"].size == (120, 80)
    assert detail["rating"] == 4
    assert {item["ext"] for item in detail["variants"]} == {"nef", "jpg", "xmp"}
    assert detail["path"].is_file()
    assert load_metadata_detail(metadata_collection, 999999) is None


def test_missing_files_excluded_and_reader_coexists_with_writer(metadata_collection):
    root = metadata_collection
    with CollectionIndex.open(root) as writer:
        (root / "DSC_0002.jpg").unlink()
        writer.quick_scan(confirm_missing=lambda *_: True)
        result = load_metadata_page(root, MetadataFilter())
        assert result["summary"]["total"] == 3
        assert result["cameras"] == ["Nikon Z6 II"]


def test_unindexed_empty_and_error_states(tmp_path):
    result = load_metadata_page(tmp_path, MetadataFilter())
    assert not result["indexed"] and result["summary"]["total"] == 0
    assert not list(tmp_path.iterdir())
    with CollectionIndex.open(tmp_path):
        pass
    result = load_metadata_page(tmp_path, MetadataFilter())
    assert result["indexed"] and not result["rows"]


def test_raw_embedded_preview_is_read_only(tmp_path, monkeypatch):
    data = io.BytesIO()
    Image.new("RGB", (800, 600), "green").save(data, format="JPEG")
    commands = []

    def run(command, **kwargs):
        commands.append(command)
        assert kwargs["timeout"] > 0

        class Result:
            returncode = 0
            stdout = b"" if "-PreviewImage" in command else data.getvalue()

        return Result()

    monkeypatch.setattr("core.metadata_browser.subprocess.run", run)
    path = tmp_path / "test.NEF"
    path.write_bytes(b"RAW")
    image = _raw_preview(path, "exiftool")
    assert image.width <= 480 and image.height <= 285
    assert commands == [
        ["exiftool", "-b", "-PreviewImage", str(path)],
        ["exiftool", "-b", "-JpgFromRaw", str(path)],
    ]
    assert path.read_bytes() == b"RAW"
