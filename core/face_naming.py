"""Name a whole face group atomically and undo its explicit user decisions."""

from __future__ import annotations

from dataclasses import dataclass

from core.face_service import FaceService

LABEL_FIELDS = ("person_id", "assignment", "confidence", "assigned_at", "ignored")


@dataclass
class NamingChange:
    faces: list[dict]
    rejections: list[tuple]
    training: list[tuple]
    person_id: int
    name: str
    created: bool
    expected: dict[int, tuple]


def assign_name(
    service: FaceService,
    face_ids: list[int],
    name: str,
    person_id: int | None = None,
    excluded_ids: list[int] | None = None,
) -> NamingChange:
    """Resolve an existing name or create it in the same transaction as the labels."""
    service.index._writable()
    name = name.strip()
    if not name:
        raise ValueError("Bitte einen Namen eingeben.")
    ids = sorted(set(face_ids))
    if not ids:
        raise ValueError("Bitte mindestens ein Gesicht auswählen.")
    excluded = sorted(set(excluded_ids or []) - set(ids))
    people = service.persons()
    if person_id is None:
        matches = [
            p
            for p in people
            if p["attrs"].get("name", "").strip().casefold() == name.casefold()
        ]
        if len(matches) > 1:
            raise ValueError(
                "Der Name ist mehrfach vorhanden. Bitte eine Person aus der Liste auswählen."
            )
        if matches:
            person_id = matches[0]["id"]
    else:
        service.graph._person(person_id)
        person = next(p for p in people if p["id"] == person_id)
        name = person["attrs"].get("name") or name
    faces = []
    rejections, training = [], []
    for face_id in sorted(set(ids + excluded)):
        row = service.db.execute(
            "SELECT * FROM faces WHERE id=?", (face_id,)
        ).fetchone()
        if not row:
            raise ValueError("Die Gruppe wurde geändert. Bitte aktualisieren.")
        faces.append(dict(row))
        rejections.extend(
            tuple(r)
            for r in service.db.execute(
                "SELECT * FROM face_rejections WHERE face_id=?", (face_id,)
            )
        )
        training.extend(
            tuple(r)
            for r in service.db.execute(
                "SELECT * FROM training_set WHERE face_id=?", (face_id,)
            )
        )
    created = person_id is None
    with service.db:
        if created:
            person_id = service.db.execute(
                "INSERT INTO persons DEFAULT VALUES"
            ).lastrowid
            service.db.execute(
                "INSERT INTO person_attrs VALUES(?,'name',?)", (person_id, name)
            )
        service._assign_faces(ids, person_id)
        for face_id in excluded:
            service.db.execute(
                "INSERT OR IGNORE INTO face_rejections VALUES(?,?)",
                (face_id, person_id),
            )
            service.db.execute(
                "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND person_id=? AND assignment IS NOT 'user'",
                (face_id, person_id),
            )
    change = NamingChange(faces, rejections, training, person_id, name, created, {})
    try:
        service.rebuild_training()
        service.rematch()
    except Exception:
        _restore(service, change)
        _remove_unused_created_person(service, change)
        raise
    change.expected = {
        face["id"]: tuple(
            service.db.execute(
                "SELECT " + ",".join(LABEL_FIELDS) + " FROM faces WHERE id=?",
                (face["id"],),
            ).fetchone()
        )
        for face in faces
    }
    return change


def undo_name(service: FaceService, change: NamingChange) -> None:
    """Restore labels, rejected candidates and pins, then recompute machine labels."""
    service.index._writable()
    for face in change.faces:
        current = service.db.execute(
            "SELECT " + ",".join(LABEL_FIELDS) + " FROM faces WHERE id=?", (face["id"],)
        ).fetchone()
        expected = change.expected[face["id"]]
        if (
            not current
            or (current[1] == "user" or expected[1] == "user")
            and tuple(current) != expected
        ):
            raise ValueError(
                "Die Zuordnung wurde inzwischen geändert und kann nicht rückgängig gemacht werden."
            )
    _restore(service, change)
    service.rebuild_training()
    service.rematch()
    _remove_unused_created_person(service, change)


def _remove_unused_created_person(service: FaceService, change: NamingChange) -> None:
    if (
        change.created
        and not service.db.execute(
            "SELECT 1 FROM faces WHERE person_id=?", (change.person_id,)
        ).fetchone()
        and not service.db.execute(
            "SELECT 1 FROM relations WHERE from_person=? OR to_person=?",
            (change.person_id, change.person_id),
        ).fetchone()
    ):
        with service.db:
            service.db.execute(
                "DELETE FROM face_rejections WHERE person_id=?", (change.person_id,)
            )
            service.db.execute(
                "DELETE FROM person_attrs WHERE person_id=?", (change.person_id,)
            )
            service.db.execute("DELETE FROM persons WHERE id=?", (change.person_id,))


def _restore(service: FaceService, change: NamingChange) -> None:
    with service.db:
        for face in change.faces:
            service.db.execute(
                "UPDATE faces SET "
                + ",".join(key + "=?" for key in LABEL_FIELDS)
                + " WHERE id=?",
                [face[key] for key in LABEL_FIELDS] + [face["id"]],
            )
            service.db.execute(
                "DELETE FROM face_rejections WHERE face_id=?", (face["id"],)
            )
            service.db.execute(
                "DELETE FROM training_set WHERE face_id=?", (face["id"],)
            )
        service.db.executemany(
            "INSERT INTO face_rejections VALUES(?,?)", change.rejections
        )
        service.db.executemany(
            "INSERT INTO training_set VALUES(?,?,?,?,?,?,?)", change.training
        )
