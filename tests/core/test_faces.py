"""Local face pipeline regressions with synthetic pixels and deterministic models."""

import hashlib
import json
import sqlite3
import threading
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

from core.collection_index import CollectionIndex
from core.face_algorithms import cluster_unknown, match_photo, select_references
from core.face_dataset import calibrate, export_dataset
from core.face_images import crop_face, load_image
from core.face_models import (
    Detection,
    InputSpec,
    ModelRegistry,
    align_face,
    normalize,
    prepare_input,
)
from core.face_service import FaceService


class Detector:
    id, version = "yunet", "test"

    def __init__(self):
        self.calls = 0
        self.detections = [
            Detection(
                np.array([20, 20, 100, 110]),
                0.99,
                np.array([[40, 50], [80, 50], [60, 70], [43, 90], [77, 90]]),
            )
        ]

    def detect(self, rgb):
        self.calls += 1
        return self.detections


class Embedder:
    id, version, dim = "sface", "test", 4
    input_spec = InputSpec()

    def __init__(self):
        self.calls = 0
        self.cancel = None

    def embed(self, faces):
        self.calls += 1
        if self.cancel:
            self.cancel.set()
        return np.tile(np.array([1, 0, 0, 0], np.float32), (len(faces), 1))


class Registry:
    def __init__(self):
        self.detector, self.embedder = Detector(), Embedder()
        self.models = {
            name: {
                "kind": kind,
                "thresholds": {"auto": 0.8, "suggest": 0.4, "duplicate": 0.92},
            }
            for name, kind in (
                ("yunet", "detector"),
                ("other_detector", "detector"),
                ("sface", "embedder"),
                ("other_embedder", "embedder"),
            )
        }

    def key(self, model):
        return model + "@test"

    def load(self, model):
        return (
            self.detector if self.models[model]["kind"] == "detector" else self.embedder
        )


@pytest.fixture
def service(tmp_path):
    pixels = np.random.default_rng(42).integers(0, 256, (128, 128, 3), dtype=np.uint8)
    Image.fromarray(pixels).save(tmp_path / "one.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        yield FaceService(index, Registry())


def test_migration_keeps_inventory_and_works_with_readers(service):
    assert service.db.execute("PRAGMA user_version").fetchone()[0] == 3
    assert service.db.execute("SELECT count(*) FROM files").fetchone()[0] == 1
    with CollectionIndex.open(service.index.root, readonly=True) as reader:
        assert reader.status()["readonly"]
        with pytest.raises(PermissionError):
            FaceService(reader, Registry()).create_person()


def test_scan_resume_pairing_and_no_raw_development(service):
    root = service.index.root
    (root / "one.nef").write_bytes(b"not a RAW, JPEG must be preferred")
    service.index.quick_scan()
    original = (root / "one.jpg").read_bytes()
    result = service.scan()
    assert result["done"] == 1 and not result["errors"]
    assert len(service.face_rows()) == 1
    assert service.registry.detector.calls == 1
    assert service.registry.embedder.calls == 1
    assert service.scan()["skipped"] == 1
    assert service.registry.detector.calls == 1
    assert (root / "one.jpg").read_bytes() == original
    face = service.face_rows()[0]
    assert face["bbox_x1"] == pytest.approx(20 / 128)
    assert face["quality"] >= 0.5
    vector = service.vectors()[0]["vector"]
    assert vector.dtype == np.float32 and np.linalg.norm(vector) == pytest.approx(1)


def test_cancel_then_resume_and_embedder_round_trip(service):
    cancel = threading.Event()
    service.registry.embedder.cancel = cancel
    assert service.scan(cancel=cancel)["cancelled"]
    assert not service.face_rows()
    service.registry.embedder.cancel = None
    assert service.scan()["done"] == 1
    person = service.create_person({"name": "Test"})
    face = service.face_rows()[0]["id"]
    service.assign([face], person)
    calls = service.registry.embedder.calls
    service.configure(active_embedder="other_embedder")
    assert service.scan()["done"] == 1
    assert service.registry.embedder.calls == calls + 1
    assert service.face_rows()[0]["person_id"] == person
    assert service.db.execute("SELECT count(*) FROM face_embeddings").fetchone()[0] == 2
    service.configure(active_embedder="sface")
    assert service.scan()["skipped"] == 1
    assert service.registry.embedder.calls == calls + 1


def test_detector_change_retains_user_decisions_and_backup(service):
    service.scan()
    face = service.face_rows()[0]["id"]
    person = service.create_person()
    service.assign([face], person)
    service.configure(active_detector="other_detector")
    assert list(service.index.root.glob(".myfotomanager.before-detector-*.bak"))
    service.scan()
    row = service.face_rows()[0]
    assert (
        row["id"] == face and row["person_id"] == person and row["assignment"] == "user"
    )
    # A later detector missing that face must retain the explicit label.
    service.configure(active_detector="yunet")
    service.registry.detector.detections = []
    service.scan()
    assert service.face_rows()[0]["assignment"] == "user"


def test_zero_faces_skipped_and_source_change_invalidates(service):
    service.registry.detector.detections = []
    assert service.scan()["done"] == 1
    assert service.scan()["skipped"] == 1
    service.registry.detector.detections = Detector().detections
    Image.new("RGB", (128, 128), "white").save(service.index.root / "one.jpg")
    assert service.scan()["done"] == 1
    assert len(service.face_rows()) == 1
    # Low quality is retained but cannot supply references or auto identities.
    assert service.face_rows()[0]["quality"] < 0.5


def add_face(
    service,
    photo,
    person=None,
    vector=(1, 0, 0, 0),
    quality=0.9,
    date="2020:01:01 12:00:00",
):
    source = service._source(1)
    with service.db:
        face = service.db.execute(
            """INSERT INTO faces(photo_id,source_file_id,source_relpath,
            detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,quality,person_id,assignment,captured_at)
            VALUES(?,?,?,'yunet@test',.1,.1,.8,.8,?, ?,?,?,?)""",
            (
                photo,
                source["id"],
                source["relpath"],
                json.dumps(
                    [[0.3, 0.4], [0.6, 0.4], [0.45, 0.5], [0.3, 0.6], [0.6, 0.6]]
                ),
                quality,
                person,
                "user" if person else None,
                date,
            ),
        ).lastrowid
        vector = normalize(np.array(vector))
        service.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,?,?)",
            (face, service.embedder_key, len(vector), vector.astype("<f4").tobytes()),
        )
    return face


def test_learning_reject_move_ignore_and_manual_labels_survive(service):
    p = service.create_person({"name": "A"})
    q = service.create_person({"name": "B"})
    reference = add_face(service, 1, p)
    query = add_face(service, 2)
    service.rebuild_training()
    service.rematch()
    row = service.face_rows("f.id=?", (query,))[0]
    assert row["person_id"] == p and row["assignment"] == "auto"
    service.reject(query)
    assert service.face_rows("f.id=?", (query,))[0]["person_id"] is None
    assert service.face_rows("f.id=?", (reference,))[0]["assignment"] == "user"
    service.assign([query], q)
    service.assign([query], p)
    assert service.db.execute(
        "SELECT 1 FROM face_rejections WHERE face_id=? AND person_id=?", (query, q)
    ).fetchone()
    service.ignore(query)
    assert service.face_rows("f.id=?", (query,))[0]["ignored"]
    assert not service.db.execute(
        "SELECT 1 FROM training_set WHERE face_id=?", (query,)
    ).fetchone()


def test_assignment_conflict_rolls_back_whole_cluster(service):
    person = service.create_person()
    a, b = add_face(service, 1), add_face(service, 1)
    with pytest.raises(ValueError, match="bereits"):
        service.assign([a, b], person)
    assert all(r["person_id"] is None for r in service.face_rows())


def test_match_uses_top_three_and_one_person_per_photo():
    queries = normalize(np.array([[1, 0], [0.9, 0.1]]))
    references = normalize(np.array([[1, 0], [0.95, 0.05], [0, 1]]))
    result = match_photo(
        queries, references, [7, 7, 8], [set(), set()], set(), 0.8, 0.4, [True, True]
    )
    assert sum(person == 7 for person, _, _ in result) == 1
    assert (
        match_photo(queries[:1], references, [7, 7, 8], [{7}], set(), 0.8, 0.4, [True])[
            0
        ][0]
        is None
    )
    assert (
        match_photo(
            queries[:1], references, [7, 7, 8], [set()], set(), 0.8, 0.4, [False]
        )[0][0]
        is None
    )


def test_clustering_respects_photo_constraint_and_complete_link():
    rows = [
        {"id": i, "photo_id": photo, "vector": np.array(vec)}
        for i, photo, vec in [
            (1, 1, [1, 0]),
            (2, 1, [1, 0]),
            (3, 2, [1, 0]),
            (4, 3, [0, 1]),
        ]
    ]
    assert cluster_unknown(rows, 0.8) == [[1, 3], [2], [4]]


def test_reference_limit_duplicates_years_pins_and_repeatability():
    rng = np.random.default_rng(42)
    rows = [
        {
            "id": i,
            "quality": 0.9,
            "vector": v,
            "captured_at": f"{2000 + i % 5}:01:{1 + i // 5:02d} 12:00:00",
            "pinned": i == 0,
        }
        for i, v in enumerate(normalize(rng.normal(size=(60, 32))))
    ]
    rows.append(rows[1] | {"id": 100, "quality": 0.8})
    selected = select_references(rows, limit=15, outlier_threshold=-1)
    assert selected == select_references(rows, limit=15, outlier_threshold=-1)
    assert len(selected.ref) + len(selected.val) == 15
    ids = set(selected.ref + selected.val)
    assert 0 in ids
    assert not {1, 100} <= ids
    assert {rows[i]["captured_at"][:4] for i in ids} == {
        str(y) for y in range(2000, 2005)
    }
    assert not set(selected.ref) & set(selected.val)
    assert len(selected.val) >= 2


def test_export_reproducible_without_names_and_delete_preserves_photos(service):
    person = service.create_person({"name": "PrivateName"})
    add_face(service, 1, person)
    service.rebuild_training()
    result = export_dataset(service)
    root = Path(result["path"])
    before = {
        p.relative_to(root): p.read_bytes() for p in root.rglob("*") if p.is_file()
    }
    (root / "stale.jpg").write_bytes(b"stale")
    export_dataset(service)
    assert before == {
        p.relative_to(root): p.read_bytes() for p in root.rglob("*") if p.is_file()
    }
    assert "PrivateName" not in (root / "manifest.json").read_text()
    assert all(b"PrivateName" not in data for data in before.values())
    service.delete_all()
    assert not root.exists()
    assert not service.face_rows()
    assert (service.index.root / "one.jpg").exists()
    assert service.index.status()["files"] == 1


def test_graph_derivations_cycles_inverse_symmetric_and_merge(service):
    ids = [service.create_person() for _ in range(7)]
    a, b, c, d, e, f, g = ids
    with service.db:
        service.graph.add(a, "elternteil_von", b)
        service.graph.add(c, "kind_von", a)
        service.graph.add(b, "elternteil_von", d)
        service.graph.add(c, "elternteil_von", e)
        service.graph.add(f, "geschwister_von", b)
        service.graph.add(d, "partner_von", g)
    assert service.graph.derived(b)["geschwister"] == [c, f]
    assert service.graph.derived(d)["grosseltern"] == [a]
    assert service.graph.derived(d)["cousins"] == [e]
    assert service.graph.derived(g)["partner"] == [d]
    with pytest.raises(ValueError, match="Zyklus"):
        service.graph.add(d, "elternteil_von", a)
    with pytest.raises(ValueError, match="Selbst"):
        service.graph.add(a, "partner_von", a)
    with pytest.raises(ValueError):
        service.merge(a, d)
    assert (
        service.db.execute(
            "SELECT merged_into FROM persons WHERE id=?", (a,)
        ).fetchone()[0]
        is None
    )


def test_alignment_color_order_and_normalization():
    rng = np.random.default_rng(1)
    pixels = rng.integers(0, 256, (112, 112, 3), dtype=np.uint8)
    template = np.array(
        [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ]
    )
    assert np.array_equal(align_face(pixels, template, InputSpec()), pixels)
    rgb = np.array([[[[10, 20, 30]]]], np.uint8)
    assert prepare_input(
        rgb, InputSpec(color="BGR", mean=10, std=10)
    ).ravel().tolist() == [2, 1, 0]
    with pytest.raises(ValueError):
        normalize(np.zeros(4))


def test_orientation_and_crop_coordinate_frame(tmp_path):
    image = Image.new("RGB", (80, 40), "white")
    image.paste("red", (0, 0, 40, 40))
    exif = Image.Exif()
    exif[274] = 6
    path = tmp_path / "rotated.jpg"
    image.save(path, exif=exif)
    oriented = load_image(path)
    assert oriented.size == (40, 80)
    assert oriented.getpixel((20, 10))[0] > 200
    assert crop_face(oriented, (0, 0, 1, 0.5)).size == (256, 256)


def test_registry_checksums_paths_and_model_version_isolation(tmp_path):
    path = tmp_path / "model.onnx"
    path.write_bytes(b"model")
    manifest = {
        "id": "test",
        "kind": "embedder",
        "version": "1",
        "runtime": "onnxruntime",
        "file": "model.onnx",
        "sha256": hashlib.sha256(b"model").hexdigest(),
        "dim": 4,
        "thresholds": {"auto": 0.8, "suggest": 0.4},
        "license": "test",
        "source_url": "local",
    }
    (tmp_path / "model.json").write_text(json.dumps(manifest))
    registry = ModelRegistry(tmp_path)
    assert registry.verify("test") == path
    assert not registry.errors
    original_key = registry.key("test")
    registry.models["test"]["version"] = "2"
    assert registry.key("test") != original_key
    path.write_bytes(b"tampered")
    with pytest.raises(ValueError, match="SHA-256"):
        registry.verify("test")
    manifest["file"] = "../outside.onnx"
    (tmp_path / "model.json").write_text(json.dumps(manifest))
    assert ModelRegistry(tmp_path).errors


def test_model_specific_thresholds_and_invalid_settings(service):
    service.configure(auto=0.9, suggest=0.6)
    service.configure(active_embedder="other_embedder")
    assert "auto" not in service.options
    service.configure(active_embedder="sface")
    assert service.options["auto"] == 0.9
    with pytest.raises(ValueError):
        service.configure(auto=0.1, suggest=0.9)
    assert service.options["auto"] == 0.9


def test_unreadable_photo_retry_and_scoped_scan(service):
    root = service.index.root
    (root / "broken.jpg").write_bytes(b"broken")
    folder = root / "sub"
    folder.mkdir()
    Image.open(root / "one.jpg").save(folder / "two.jpg")
    service.index.quick_scan()
    assert service.scan("sub")["done"] == 1
    result = service.scan()
    assert result["done"] == 1 and len(result["errors"]) == 1
    errors = service.db.execute(
        "SELECT * FROM face_jobs WHERE status='error'"
    ).fetchall()
    assert len(errors) == 1
    Image.open(root / "one.jpg").save(root / "broken.jpg")
    assert service.scan()["done"] == 1
    assert not service.db.execute(
        "SELECT * FROM face_jobs WHERE status='error'"
    ).fetchall()
    with pytest.raises(ValueError):
        service.scan("../outside")


def test_pins_limit_and_delete_scrubs_own_backups(service):
    person = service.create_person()
    a = add_face(service, 1, person)
    b = add_face(service, 2, person, vector=(0, 1, 0, 0), date="2021:01:01 12:00:00")
    service.configure(reference_limit=1)
    service.pin(a)
    with pytest.raises(ValueError, match="Referenzlimit"):
        service.pin(b)
    assert not service.db.execute(
        "SELECT * FROM training_set WHERE face_id=? AND pinned=1", (b,)
    ).fetchone()
    service.index.backup()
    service.configure(active_detector="other_detector")
    service.delete_all()
    backups = [service.index.root / ".myfotomanager.bak"] + list(
        service.index.root.glob(".myfotomanager.before-detector-*.bak")
    )
    for path in backups:
        with sqlite3.connect(path) as db:
            assert db.execute("SELECT count(*) FROM faces").fetchone()[0] == 0
            assert db.execute("SELECT count(*) FROM files").fetchone()[0] == 1


def test_calibration_uses_validation_and_separates_models(service):
    a, b = service.create_person(), service.create_person()
    for person, shift in ((a, 0), (b, 2)):
        for i in range(6):
            vector = np.zeros(4)
            vector[shift] = 1
            vector[shift + 1] = 0.2 * i
            add_face(
                service,
                1 + person * 10 + i,
                person,
                vector=vector,
                date=f"{2000 + i}:01:01 12:00:00",
            )
    service.configure(duplicate=0.999)
    service.rebuild_training()
    result = calibrate(service)
    assert result["available"] and result["samples"] >= 4
    assert result["proposed_auto"] is not None
    assert result["genuine"] and result["impostor"]
    service.configure(active_embedder="other_embedder")
    assert not calibrate(service)["available"]


def test_cooccurrence_priors_affect_only_gray_zone():
    refs = np.array([[1, 0]], dtype=np.float32)
    query = normalize(np.array([[0.39, (1 - 0.39**2) ** 0.5]], dtype=np.float32))
    assert match_photo(query, refs, [1], [set()], set(), 0.8, 0.4, [True])[0][0] is None
    assert (
        match_photo(
            query, refs, [1], [set()], set(), 0.8, 0.4, [True], priors={1: 0.03}
        )[0][1]
        == "suggested"
    )
    clear = np.array([[1, 0]], dtype=np.float32)
    assert (
        match_photo(
            clear, refs, [1], [set()], set(), 0.8, 0.4, [True], priors={1: -0.5}
        )[0][1]
        == "auto"
    )


@pytest.mark.parametrize("orientation", range(1, 9))
def test_all_exif_orientation_coordinate_frames(tmp_path, orientation):
    image = Image.fromarray(np.arange(60 * 40 * 3, dtype=np.uint8).reshape(40, 60, 3))
    exif = Image.Exif()
    exif[274] = orientation
    path = tmp_path / "image.png"
    image.save(path, exif=exif)
    result = load_image(path)
    operations = {
        2: Image.Transpose.FLIP_LEFT_RIGHT,
        3: Image.Transpose.ROTATE_180,
        4: Image.Transpose.FLIP_TOP_BOTTOM,
        5: Image.Transpose.TRANSPOSE,
        6: Image.Transpose.ROTATE_270,
        7: Image.Transpose.TRANSVERSE,
        8: Image.Transpose.ROTATE_90,
    }
    expected = image.transpose(operations[orientation]) if orientation != 1 else image
    assert np.array_equal(np.array(result), np.array(expected))


def test_raw_uses_jpeg_and_metadata_orientation_without_developing(
    tmp_path, monkeypatch
):
    from io import BytesIO
    from types import SimpleNamespace

    buffer = BytesIO()
    Image.new("RGB", (80, 40), "red").save(buffer, format="JPEG")
    calls = []

    def run(args, **kwargs):
        calls.append(args)
        return SimpleNamespace(returncode=0, stdout=buffer.getvalue())

    monkeypatch.setattr("core.face_images.subprocess.run", run)
    image = load_image(tmp_path / "one.nef", orientation=6)
    assert image.size == (40, 80)
    assert len(calls) == 1 and "-JpgFromRaw" in calls[0]


def test_alignment_known_translation_and_scale():
    rgb = np.random.default_rng(3).integers(0, 256, (112, 112, 3), dtype=np.uint8)
    template = np.array(
        [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ]
    )
    shifted = np.zeros((244, 244, 3), dtype=np.uint8)
    import cv2

    shifted[10:234, 10:234] = cv2.resize(
        rgb, (224, 224), interpolation=cv2.INTER_NEAREST
    )
    aligned = align_face(shifted, template * 2 + 10, InputSpec())
    assert np.array_equal(aligned, rgb)


def test_training_exclusion_blocks_fallback_and_pin_restores_it(service):
    person = service.create_person()
    reference = add_face(service, 1, person)
    query = add_face(service, 2)
    service.rebuild_training()
    service.exclude_training(reference)
    assert service.face_rows("f.id=?", (query,))[0]["person_id"] is None
    assert service.face_rows("f.id=?", (reference,))[0]["assignment"] == "user"
    service.pin(reference)
    assert service.face_rows("f.id=?", (query,))[0]["person_id"] == person


def test_merge_preserves_pins_and_redirects_faces(service):
    a, b = service.create_person(), service.create_person()
    one = add_face(service, 1, a)
    two = add_face(service, 2, b, vector=(0, 1, 0, 0), date="2021:01:01 12:00:00")
    service.pin(one)
    service.pin(two)
    service.merge(a, b)
    assert (
        service.db.execute(
            "SELECT merged_into FROM persons WHERE id=?", (a,)
        ).fetchone()[0]
        == b
    )
    assert all(r["person_id"] == b for r in service.face_rows())
    assert (
        service.db.execute(
            "SELECT count(*) FROM training_set WHERE person_id=? AND pinned=1", (b,)
        ).fetchone()[0]
        == 2
    )


def test_upgrade_inventory_v1_and_face_v2_keep_ids(service):
    root = service.index.root
    service.scan()
    photo = service.face_rows()[0]["photo_id"]
    # Reproduce the first face-schema checkpoint on a separate DB snapshot.
    other = root / "upgrade"
    other.mkdir()
    with sqlite3.connect(other / ".myfotomanager") as db:
        service.db.backup(db)
        db.execute("DROP TABLE training_exclusions")
        db.execute("PRAGMA user_version=2")
    with CollectionIndex.open(other) as index:
        assert index.db.execute("PRAGMA user_version").fetchone()[0] == 3
        assert index.db.execute("SELECT photo_id FROM faces").fetchone()[0] == photo
