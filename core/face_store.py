"""Versioned face data in the collection DB; all writes use its writer lock."""
from __future__ import annotations

import json
import sqlite3


def migrate(db: sqlite3.Connection) -> None:
    """Extend inventory v1 atomically without replacing its photos or file IDs."""
    if db.execute("PRAGMA user_version").fetchone()[0] != 1:
        return
    db.executescript("""
        BEGIN IMMEDIATE;
        CREATE TABLE face_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE persons(id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, merged_into INTEGER);
        CREATE TABLE person_attrs(person_id INTEGER, key TEXT, value TEXT,
            PRIMARY KEY(person_id,key));
        CREATE TABLE faces(id INTEGER PRIMARY KEY AUTOINCREMENT, photo_id INTEGER NOT NULL,
            source_file_id INTEGER, source_relpath TEXT NOT NULL,
            detector_id TEXT NOT NULL, bbox_x1 REAL, bbox_y1 REAL, bbox_x2 REAL, bbox_y2 REAL,
            landmarks TEXT, det_score REAL, quality REAL, person_id INTEGER,
            assignment TEXT CHECK(assignment IN ('user','auto','suggested') OR assignment IS NULL),
            confidence REAL, assigned_at TEXT, ignored INTEGER NOT NULL DEFAULT 0,
            retained INTEGER NOT NULL DEFAULT 0, captured_at TEXT, source_fingerprint TEXT);
        CREATE INDEX ix_faces_photo ON faces(photo_id);
        CREATE INDEX ix_faces_person ON faces(person_id);
        CREATE TABLE face_embeddings(face_id INTEGER, embedder_id TEXT, dim INTEGER, vec BLOB,
            PRIMARY KEY(face_id,embedder_id));
        CREATE TABLE face_rejections(face_id INTEGER, person_id INTEGER,
            PRIMARY KEY(face_id,person_id));
        CREATE TABLE training_set(person_id INTEGER, face_id INTEGER, embedder_id TEXT,
            split TEXT CHECK(split IN ('ref','val')), selected_at TEXT, reason TEXT,
            pinned INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(person_id,face_id,embedder_id));
        CREATE TABLE face_review(face_id INTEGER, embedder_id TEXT, reason TEXT,
            PRIMARY KEY(face_id,embedder_id));
        CREATE TABLE relation_types(id INTEGER PRIMARY KEY, name TEXT UNIQUE,
            inverse_id INTEGER, symmetric INTEGER);
        INSERT INTO relation_types VALUES(1,'elternteil_von',2,0),(2,'kind_von',1,0),
            (3,'partner_von',3,1),(4,'geschwister_von',4,1);
        CREATE TABLE relations(from_person INTEGER, type_id INTEGER, to_person INTEGER,
            PRIMARY KEY(from_person,type_id,to_person));
        CREATE TABLE face_jobs(photo_id INTEGER, stage TEXT, model_id TEXT, status TEXT,
            error TEXT, updated_at TEXT, fingerprint TEXT,
            PRIMARY KEY(photo_id,stage,model_id));
        PRAGMA user_version=2;
        COMMIT;
    """)


def settings(db: sqlite3.Connection) -> dict:
    from config.settings import FACE_DEFAULTS
    return dict(FACE_DEFAULTS) | {
        row[0]: json.loads(row[1]) for row in db.execute("SELECT * FROM face_settings")
    }


def save_settings(db: sqlite3.Connection, values: dict) -> None:
    """Persist validated service settings inside the caller's transaction."""
    db.executemany("INSERT OR REPLACE INTO face_settings VALUES(?,?)",
                   [(key, json.dumps(value)) for key, value in values.items()])
