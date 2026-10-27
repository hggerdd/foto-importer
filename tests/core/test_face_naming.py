"""Group naming and undo preserve labels, rejections and person identity."""

import json

import numpy as np
import pytest
from PIL import Image

from core.collection_index import CollectionIndex
from core.face_naming import assign_name, undo_name
from core.face_service import FaceService


@pytest.fixture
def service(tmp_path):
    for number in range(4):
        Image.new("RGB", (80, 80), "white").save(tmp_path / f"{number}.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
        yield FaceService(index)


def face(service, source=1, person=None, vector=(1, 0, 0, 0)):
    file = service.db.execute("SELECT * FROM files WHERE id=?", (source,)).fetchone()
    with service.db:
        fid = service.db.execute(
            """INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,
            bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,person_id,assignment,landmarks)
            VALUES(?,?,?,'test',.1,.1,.9,.9,.9,?,?,?)""",
            (
                file["photo_id"],
                file["id"],
                file["relpath"],
                person,
                "user" if person else None,
                json.dumps(
                    [[0.3, 0.3], [0.7, 0.3], [0.5, 0.5], [0.3, 0.7], [0.7, 0.7]]
                ),
            ),
        ).lastrowid
        array = np.array(vector, dtype="<f4")
        service.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,?,?)",
            (fid, service.embedder_key, len(array), array.tobytes()),
        )
    return fid


def test_one_step_creates_name_and_undo_restores_unknown_group(service):
    ids = [face(service, 1), face(service, 2)]
    change = assign_name(service, ids, "  Otto  ")
    assert change.created and change.name == "Otto"
    assert len(service.persons()) == 1
    assert all(
        row["assignment"] == "user" and row["person_id"] == change.person_id
        for row in service.face_rows()
    )
    undo_name(service, change)
    assert all(row["person_id"] is None for row in service.face_rows())
    assert not service.persons()
    assert not service.db.execute("SELECT * FROM training_set").fetchall()


def test_reuses_existing_casefolded_name_and_allows_disambiguation(service):
    person = service.create_person({"name": "JÖRG"})
    fid = face(service)
    change = assign_name(service, [fid], " jörg ")
    assert not change.created and change.person_id == person
    service.create_person({"name": "Jörg"})
    with pytest.raises(ValueError, match="mehrfach"):
        assign_name(service, [fid], "jörg")
    assert assign_name(service, [fid], "JÖRG", person).person_id == person


def test_conflict_does_not_leave_new_person_or_partial_labels(service):
    ids = [face(service), face(service)]
    with pytest.raises(ValueError, match="bereits"):
        assign_name(service, ids, "Otto")
    assert not service.persons()
    assert all(row["person_id"] is None for row in service.face_rows())
    with pytest.raises(ValueError):
        assign_name(service, ids, "   ")
    with pytest.raises(ValueError):
        assign_name(service, [], "Otto")


def test_undo_restores_old_person_pins_and_rejections(service):
    old, new = (
        service.create_person({"name": "Anna"}),
        service.create_person({"name": "Marie"}),
    )
    fid = face(service, person=old)
    service.pin(fid)
    with service.db:
        service.db.execute("INSERT INTO face_rejections VALUES(?,?)", (fid, new))
    change = assign_name(service, [fid], "Marie", new)
    assert service.face_rows()[0]["person_id"] == new
    undo_name(service, change)
    assert service.face_rows()[0]["person_id"] == old
    assert (
        service.db.execute(
            "SELECT pinned FROM training_set WHERE face_id=?", (fid,)
        ).fetchone()[0]
        == 1
    )
    assert (
        service.db.execute(
            "SELECT person_id FROM face_rejections WHERE face_id=?", (fid,)
        ).fetchone()[0]
        == new
    )
    service.assign([fid], new)
    with pytest.raises(ValueError, match="inzwischen"):
        undo_name(service, change)


def test_undo_recomputes_automatic_assignments_without_label_drift(service):
    reference = face(service, 1)
    query = face(service, 2)
    change = assign_name(service, [reference], "Otto")
    assert service.face_rows("f.id=?", (query,))[0]["assignment"] == "auto"
    undo_name(service, change)
    assert all(row["person_id"] is None for row in service.face_rows())


def test_unchecked_outlier_is_rejected_and_cannot_be_auto_assigned_again(service):
    reference, outlier = face(service, 1), face(service, 2)
    change = assign_name(service, [reference], "Otto", excluded_ids=[outlier])
    assert service.face_rows("f.id=?", (outlier,))[0]["person_id"] is None
    assert service.db.execute(
        "SELECT 1 FROM face_rejections WHERE face_id=? AND person_id=?",
        (outlier, change.person_id),
    ).fetchone()
    undo_name(service, change)
    assert not service.db.execute("SELECT * FROM face_rejections").fetchall()
    assert not service.persons()


def test_failed_reference_rebuild_does_not_leave_labels_or_an_empty_new_person(
    service, monkeypatch
):
    fid = face(service)

    def fail():
        raise RuntimeError("Reference rebuild failed")

    monkeypatch.setattr(service, "rebuild_training", fail)
    with pytest.raises(RuntimeError):
        assign_name(service, [fid], "Otto")
    assert service.face_rows()[0]["person_id"] is None
    assert not service.persons()
