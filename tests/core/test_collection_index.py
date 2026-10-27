"""Regression coverage for collection identities and destructive-scan guards."""

import json
import os
import sqlite3
import threading
import unicodedata
from contextlib import closing

import pytest

from core.collection_index import CollectionIndex
from core.collection_metadata import identity_key, read_metadata


def photo(root, name="album/nef/DSC_0001.nef", content=b"photo"):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    os.utime(path, ns=(1_700_000_000_000_000_000,) * 2)
    return path


def row(index, path=None):
    return dict(
        index.db.execute(
            "SELECT * FROM files" + (" WHERE relpath=?" if path else ""),
            (path,) if path else (),
        ).fetchone()
    )


def test_inventory_pairing_exclusions_backup(tmp_path):
    photo(tmp_path)
    photo(tmp_path, "album/jpg/DSC_0001.JPG")
    photo(tmp_path, "album/nef/DSC_0001.nef.xmp")
    photo(tmp_path, ".hidden/photo.jpg")
    photo(tmp_path, "@eaDir/photo.jpg")
    photo(tmp_path, "excluded/photo.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.configure(exclusions=["excluded/*"])
        result = index.quick_scan()
        assert result.stats["added"] == 3
        assert index.status()["photos"] == 1
        assert len(index.photo_files(1)) == 3
        assert index.quick_scan().stats["added"] == 0
        assert len(list(index.changes_since(result.scan_id))) == 0
        with closing(sqlite3.connect(tmp_path / ".myfotomanager.bak")) as backup:
            assert backup.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert index.db.execute("PRAGMA journal_mode").fetchone()[0] == "wal"


def test_rename_keeps_file_and_photo_ids(tmp_path):
    path = photo(tmp_path)
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        original = row(index)
        path.rename(path.with_name("renamed.nef"))
        result = index.quick_scan()
        assert result.stats["moved"] == 1
        assert result.stats["added"] == result.stats["missing"] == 0
        assert row(index)["id"] == original["id"]
        assert row(index)["photo_id"] == original["photo_id"]


def test_folder_rename_does_not_hash(tmp_path, monkeypatch):
    for number in range(100):
        photo(tmp_path, f"old/{number}.jpg", str(number).encode())
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        before = {
            item[0]: item[1]
            for item in index.db.execute("SELECT id,photo_id FROM files")
        }
        (tmp_path / "old").rename(tmp_path / "new")
        monkeypatch.setattr(
            index,
            "_hash",
            lambda *args, **kwargs: pytest.fail("folder move must not hash"),
        )
        result = index.quick_scan()
        assert result.stats["moved"] == 100
        assert result.stats["renamed_folders"] == [{"old": "old", "new": "new"}]
        assert {
            item[0]: item[1]
            for item in index.db.execute("SELECT id,photo_id FROM files")
        } == before


def test_missing_returned_and_modified(tmp_path):
    path = photo(tmp_path)
    photo(tmp_path, "keep.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        before = row(index, "album/nef/DSC_0001.nef")
        path.unlink()
        guarded = index.quick_scan()
        assert guarded.status == "needs_confirmation"
        assert row(index, before["relpath"])["status"] == "present"
        assert index.quick_scan(confirm_missing=lambda *_: True).stats["missing"] == 1
        assert row(index, before["relpath"])["photo_id"] == before["photo_id"]
        photo(tmp_path)
        assert index.quick_scan().stats["returned"] == 1
        path.write_bytes(b"other")
        os.utime(path, ns=(1_700_000_010_000_000_000,) * 2)
        index.db.execute(
            "UPDATE files SET meta_state='done',full_hash='old',quick_hash='old'"
        )
        index.db.commit()
        assert index.quick_scan().stats["modified"] == 1
        updated = row(index, before["relpath"])
        assert updated["meta_state"] == "stale"
        assert updated["quick_hash"] is updated["full_hash"] is None


def test_empty_cancelled_and_scope_preserve_unseen(tmp_path):
    path = photo(tmp_path)
    other = photo(tmp_path, "other/keep.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        path.unlink()
        assert index.quick_scan("album").status == "error"
        assert index.status()["missing"] == 0
        event = threading.Event()
        event.set()
        assert index.quick_scan(cancel=event).status == "cancelled"
        assert index.status()["missing"] == 0
        photo(tmp_path, "album/keep.jpg", b"different")
        index.quick_scan("album", confirm_missing=lambda *_: True)
        assert row(index, "other/keep.jpg")["status"] == "present"
        other.unlink()
        assert index.quick_scan("album").stats["missing"] == 0
        with pytest.raises(ValueError):
            index.quick_scan("../")


def test_unicode_and_case_rename(tmp_path):
    name = unicodedata.normalize("NFD", "Wörthersee.jpg")
    path = photo(tmp_path, name)
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        assert row(index)["relpath"] == "Wörthersee.jpg"
        assert index.verify().stats["processed"] == 1
        path.rename(tmp_path / "WÖRTHERSEE.JPG")
        assert index.quick_scan().stats["moved"] == 1
        assert index.status()["files"] == 1


def test_verification_preserves_corruption_evidence(tmp_path):
    path = photo(tmp_path)
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        index.verify()
        digest = row(index)["full_hash"]
        mtime = path.stat().st_mtime_ns
        path.write_bytes(b"other")
        os.utime(path, ns=(mtime, mtime))
        assert index.verify().stats["mismatches"] == ["album/nef/DSC_0001.nef"]
        assert row(index)["full_hash"] == digest
        assert index.verify().stats["mismatches"]


def test_single_writer_and_collection_uuid(tmp_path):
    photo(tmp_path)
    with CollectionIndex.open(tmp_path) as first:
        first.quick_scan()
        collection = first.status()["collection_id"]
        with CollectionIndex.open(tmp_path) as second:
            assert second.readonly
            with pytest.raises(PermissionError):
                second.quick_scan()
    assert not (tmp_path / ".myfotomanager.lock").exists()
    renamed = tmp_path.with_name(tmp_path.name + "-moved")
    tmp_path.rename(renamed)
    try:
        with CollectionIndex.open(renamed) as index:
            assert index.status()["collection_id"] == collection
    finally:
        renamed.rename(tmp_path)


def fake_metadata(paths, executable="exiftool"):
    return [
        {
            "SourceFile": str(path),
            "IFD0:Make": "NIKON",
            "IFD0:Model": "Z6",
            "Nikon:SerialNumber": "123",
            "Nikon:ShutterCount": int(path.read_bytes().decode()),
            "EXIF:DateTimeOriginal": "2025:07:14 12:00:00",
        }
        for path in paths
    ]


def test_metadata_identity_import_and_merge(tmp_path, monkeypatch):
    monkeypatch.setattr("core.collection_index.read_metadata", fake_metadata)
    root = tmp_path / "collection"
    root.mkdir()
    photo(root, "a/renamed.nef", b"1")
    photo(root, "b/DSC_0001.jpg", b"1")
    photo(root, "Export/export.jpg", b"1")
    card = photo(tmp_path, "card/DSC_0001.nef", b"1")
    different = photo(tmp_path, "card2/DSC_0001.jpg", b"2")
    with CollectionIndex.open(root) as index:
        index.quick_scan()
        ids = [
            item[0]
            for item in index.db.execute(
                "SELECT photo_id FROM files WHERE dir!='Export'"
            )
        ]
        assert index.enrich_metadata().stats["processed"] == 3
        assert index.status()["photos"] == 2  # merged capture + independent export
        assert row(index, "a/renamed.nef")["photo_id"] == min(ids)
        assert index.check_import([card])[0].status == "imported"
        assert index.check_import([different])[0].status == "new"
        (root / "a/renamed.nef").unlink()
        (root / "b/DSC_0001.jpg").unlink()
        index.quick_scan(confirm_missing=lambda *_: True)
        assert index.check_import([card])[0].status == "imported_missing"
        assert index.purge_missing() == 2
        assert index.db.execute("SELECT COUNT(*) FROM photos").fetchone()[0] == 3


def test_pending_likely_and_identity_collision(tmp_path, monkeypatch):
    monkeypatch.setattr("core.collection_index.read_metadata", fake_metadata)
    root = tmp_path / "collection"
    root.mkdir()
    photo(root, "DSC_0001.nef", b"1")
    card = photo(tmp_path, "card/DSC_0001.nef", b"1")
    with CollectionIndex.open(root) as index:
        index.quick_scan()
        assert index.check_import([card])[0].status == "imported_likely"
        index.enrich_metadata()
        card.write_bytes(b"2")
        assert index.check_import([card])[0].status == "new"


def test_identity_tiers():
    assert identity_key(dict(serial="42", shutter_count=0)) == "42:0"
    assert identity_key(dict(serial="42", dt_original="time")) == "42:time."
    weak = dict(make="Nikon", model="Z6", dt_original="time", width=6000, height=4000)
    assert identity_key(weak) is None
    assert identity_key(dict(weak, subsec="123")) == "NikonZ6:time.123:6000x4000"


def test_metadata_batch_command_keeps_makernotes(tmp_path, monkeypatch):
    paths = [photo(tmp_path, "ü name.jpg")]

    def run(command, **kwargs):
        assert "-fast" in command and "-fast2" not in command
        assert "-@" in command
        assert paths[0].name.encode() in kwargs["input"]

        class Result:
            returncode = 0
            stdout = json.dumps([{"SourceFile": str(paths[0])}]).encode()

        return Result()

    monkeypatch.setattr("core.collection_metadata.subprocess.run", run)
    assert len(read_metadata(paths)) == 1


def test_metadata_failure_and_resume(tmp_path, monkeypatch):
    photo(tmp_path)
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()

        def fail(*args):
            raise RuntimeError("ExifTool fehlt")

        monkeypatch.setattr("core.collection_index.read_metadata", fail)
        assert index.enrich_metadata().status == "error"
        assert row(index)["meta_state"] == "pending"
        monkeypatch.setattr(
            "core.collection_index.read_metadata",
            lambda paths, *_: [
                {"SourceFile": str(path), "Error": "bad image"} for path in paths
            ],
        )
        assert index.enrich_metadata().stats["errors_count"] == 1
        assert row(index)["meta_state"] == "error"


def test_ambiguous_hash_moves_require_review(tmp_path):
    photo(tmp_path, "a/old.jpg")
    photo(tmp_path, "b/old.jpg")
    photo(tmp_path, "keep.jpg", b"different")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        index.verify()
        original = row(index, "a/old.jpg")
        (tmp_path / "a/old.jpg").unlink()
        (tmp_path / "b/old.jpg").unlink()
        photo(tmp_path, "c/new.jpg")
        result = index.quick_scan(confirm_missing=lambda *_: True)
        assert len(result.stats["ambiguous"]) == 1
        index.resolve_move("a/old.jpg", "c/new.jpg")
        assert row(index, "c/new.jpg")["id"] == original["id"]


def test_unreadable_tree_never_marks_missing(tmp_path, monkeypatch):
    photo(tmp_path)
    photo(tmp_path, "keep.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        scan = os.scandir

        def denied(path):
            if str(path).endswith("album"):
                raise PermissionError("denied")
            return scan(path)

        monkeypatch.setattr("core.collection_index.os.scandir", denied)
        result = index.quick_scan()
        assert result.errors
        assert index.status()["missing"] == 0
