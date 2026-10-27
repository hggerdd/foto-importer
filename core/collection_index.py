"""Persistent, read-only photo collection inventory and stable photo identities.

Only .myfotomanager and its companion files are written. Each instance belongs
to its creating thread; use a fresh instance inside background jobs.
"""

from __future__ import annotations

import fnmatch
import json
import os
import posixpath
import socket
import sqlite3
import time
import unicodedata
import uuid
from collections import defaultdict
from collections.abc import Iterator
from contextlib import closing
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

from blake3 import blake3

from core.collection_metadata import identity_key, metadata_values, read_metadata
from core.windows_file_ids import directory_file_ids


EXTENSIONS = "nef nrw dng jpg jpeg heic tif tiff png mov mp4".split()
SYSTEM_DIRS = {"$recycle.bin", "system volume information", "@eadir", ".trashes"}
TOLERANCE_NS = 2_000_000_000


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def normalized(path: str) -> str:
    return unicodedata.normalize("NFC", path.replace("\\", "/"))


def in_scope(path: str, scope: str) -> bool:
    return not scope or path.startswith(scope + "/") or path == scope


def stopped(cancel) -> bool:
    return cancel is not None and cancel.is_set()


@dataclass
class ScanResult:
    scan_id: int
    status: str = "running"
    stats: dict = field(default_factory=dict)
    errors: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class ImportStatus:
    card_path: Path
    status: str
    photo_id: int | None = None
    collection_relpath: str | None = None


class CollectionIndex:
    """SQLite cache with one process writer lock and safe manual scans."""

    @classmethod
    def open(cls, root: Path, *, readonly: bool = False) -> CollectionIndex:
        return cls(root, readonly=readonly)

    def __init__(self, root: Path, *, readonly: bool = False) -> None:
        self.root = Path(root).resolve(strict=True)
        if not self.root.is_dir():
            raise ValueError("Sammlung ist kein Ordner")
        self.path = self.root / ".myfotomanager"
        self.lock_path = self.root / ".myfotomanager.lock"
        self._lock_fd = None
        self._physical: dict[str, Path] = {}
        if not readonly:
            try:
                self._lock_fd = os.open(
                    self.lock_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY
                )
                os.write(
                    self._lock_fd,
                    json.dumps(
                        {"host": socket.gethostname(), "pid": os.getpid()}
                    ).encode(),
                )
            except FileExistsError:
                readonly = True
        self.readonly = readonly
        try:
            if readonly:
                self.db = sqlite3.connect(self.path.as_uri() + "?mode=ro", uri=True)
            else:
                self.db = sqlite3.connect(self.path)
            self.db.row_factory = sqlite3.Row
            self.db.execute("PRAGMA busy_timeout=5000")
            if not readonly:
                for pragma in (
                    "journal_mode=WAL",
                    "synchronous=NORMAL",
                    "temp_store=MEMORY",
                    "cache_size=-65536",
                ):
                    self.db.execute("PRAGMA " + pragma)
                self._schema()
            if self.db.execute("PRAGMA user_version").fetchone()[0] != 1:
                raise ValueError("Nicht unterstützte Index-Version")
            self.settings = {
                row[0]: json.loads(row[1])
                for row in self.db.execute("SELECT * FROM settings")
            }
        except BaseException:
            if hasattr(self, "db"):
                self.db.close()
            self._unlock()
            raise

    def _schema(self) -> None:
        version = self.db.execute("PRAGMA user_version").fetchone()[0]
        if version != 0:
            return
        self.db.executescript("""
            CREATE TABLE collection(id TEXT PRIMARY KEY, created_at TEXT, app_version TEXT);
            CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE photos(id INTEGER PRIMARY KEY AUTOINCREMENT, identity_key TEXT UNIQUE,
                status TEXT NOT NULL, created_scan INTEGER);
            CREATE TABLE files(id INTEGER PRIMARY KEY AUTOINCREMENT,
                relpath TEXT NOT NULL UNIQUE, relpath_fold TEXT NOT NULL, dir TEXT NOT NULL,
                name TEXT NOT NULL, stem TEXT NOT NULL, ext TEXT NOT NULL, size INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL, inode TEXT, quick_hash TEXT, full_hash TEXT,
                photo_id INTEGER, status TEXT NOT NULL, first_seen_scan INTEGER,
                last_seen_scan INTEGER, meta_state TEXT NOT NULL DEFAULT 'pending', meta_error TEXT);
            CREATE TABLE meta(file_id INTEGER PRIMARY KEY, make TEXT, model TEXT, serial TEXT,
                shutter_count INTEGER, dt_original TEXT, subsec TEXT, offset_time TEXT,
                width INTEGER, height INTEGER, orientation INTEGER, lens TEXT, focal REAL,
                aperture REAL, exposure TEXT, iso INTEGER, lat REAL, lon REAL, alt REAL,
                rating INTEGER, label TEXT, json TEXT);
            CREATE TABLE scans(id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT, scope TEXT,
                started_at TEXT, finished_at TEXT, status TEXT, stats_json TEXT);
            CREATE TABLE changes(id INTEGER PRIMARY KEY AUTOINCREMENT, scan_id INTEGER,
                kind TEXT, file_id INTEGER, photo_id INTEGER, old_relpath TEXT, new_relpath TEXT, at TEXT);
            PRAGMA user_version=1;
        """)
        self.db.execute(
            "INSERT INTO collection VALUES(?,?,?)", (str(uuid.uuid4()), now(), "0.1.0")
        )
        defaults = dict(
            extensions=EXTENSIONS,
            exclusions=[],
            export_folders=["Export"],
            extension_folders=EXTENSIONS,
            exiftool="exiftool",
            auto_metadata=False,
        )
        self.db.executemany(
            "INSERT INTO settings VALUES(?,?)",
            [(k, json.dumps(v)) for k, v in defaults.items()],
        )
        self.db.commit()

    def _indexes(self) -> None:
        for name, table, columns in (
            ("name_size", "files", "name,size"),
            ("size", "files", "size"),
            ("stem_dir", "files", "dir,stem"),
            ("photo", "files", "photo_id"),
            ("fold", "files", "relpath_fold"),
            ("identity", "meta", "serial,shutter_count"),
            ("time", "meta", "dt_original,subsec"),
            ("scan", "changes", "scan_id"),
        ):
            self.db.execute(
                f"CREATE INDEX IF NOT EXISTS ix_{table}_{name} ON {table}({columns})"
            )

    def _unlock(self) -> None:
        if self._lock_fd is not None:
            os.close(self._lock_fd)
            self._lock_fd = None
            self.lock_path.unlink(missing_ok=True)

    def close(self) -> None:
        self.db.close()
        self._unlock()

    def __enter__(self) -> CollectionIndex:
        return self

    def __exit__(self, *args) -> None:
        self.close()

    def _writable(self) -> None:
        if self.readonly:
            raise PermissionError(
                "Sammlung durch eine andere Instanz gesperrt (nur lesbar)"
            )

    def configure(self, **settings) -> None:
        """Persist collection options (extensions, exclusions, export folders, etc.)."""
        self._writable()
        self.settings.update(settings)
        with self.db:
            self.db.executemany(
                "INSERT OR REPLACE INTO settings VALUES(?,?)",
                [(k, json.dumps(v)) for k, v in settings.items()],
            )

    def _scope(self, scope: str) -> str:
        scope = normalized(scope)
        if (
            PurePosixPath(scope).is_absolute()
            or ".." in PurePosixPath(scope).parts
            or ":" in scope
        ):
            raise ValueError("Scope muss relativ zur Sammlung sein")
        scope = scope.strip("/")
        if scope == ".":
            scope = ""
        actual = self._path(scope)
        resolved = actual.resolve(strict=True)
        if not resolved.is_relative_to(self.root) or not resolved.is_dir():
            raise ValueError("Scope liegt außerhalb der Sammlung")
        if actual.is_symlink():
            raise ValueError("Symlinks werden nicht gescannt")
        return scope

    def _path(self, relpath: str) -> Path:
        if relpath in self._physical:
            return self._physical[relpath]
        path = self.root / relpath
        if path.exists():
            return path
        # NFC is stored even on an NFD filesystem. Resolve each component lazily.
        path = self.root
        for component in PurePosixPath(relpath).parts:
            direct = path / component
            if direct.exists():
                path = direct
            else:
                path = next(
                    (
                        child
                        for child in path.iterdir()
                        if normalized(child.name) == component
                    ),
                    direct,
                )
        return path

    def _begin(self, kind: str, scope: str = "") -> ScanResult:
        self._writable()
        cursor = self.db.execute(
            "INSERT INTO scans(kind,scope,started_at,status) VALUES(?,?,?,'running')",
            (kind, scope, now()),
        )
        self.db.commit()
        return ScanResult(cursor.lastrowid)

    def _finish(self, result: ScanResult, status: str = "done") -> ScanResult:
        result.status = status
        result.stats["errors"] = result.errors
        self._indexes()
        self._refresh_photos()
        self.db.execute(
            "UPDATE scans SET finished_at=?,status=?,stats_json=? WHERE id=?",
            (now(), status, json.dumps(result.stats), result.scan_id),
        )
        self._indexes()
        self.db.commit()
        self.backup()
        return result

    def backup(self) -> None:
        """Create a consistent SQLite snapshot after a completed operation."""
        self._writable()
        with closing(sqlite3.connect(self.root / ".myfotomanager.bak")) as destination:
            self.db.backup(destination)

    def _change(self, scan_id: int, kind: str, row, old=None, new=None) -> None:
        self.db.execute(
            "INSERT INTO changes(scan_id,kind,file_id,photo_id,old_relpath,new_relpath,at) VALUES(?,?,?,?,?,?,?)",
            (scan_id, kind, row["id"], row["photo_id"], old, new, now()),
        )

    def _refresh_photos(self) -> None:
        self.db.execute("""UPDATE photos SET status=CASE WHEN EXISTS
            (SELECT 1 FROM files WHERE files.photo_id=photos.id AND files.status='present' AND ext!='xmp')
            THEN 'present' ELSE 'missing' END""")

    def _walk(self, scope: str, result: ScanResult, cancel):
        actual = self._path(scope)
        stack = [(actual, actual.relative_to(self.root).as_posix() if scope else "")]
        extensions = {*self.settings["extensions"], "xmp"}
        while stack and not stopped(cancel):
            folder, folder_rel = stack.pop()
            try:
                device = folder.stat().st_dev
                file_ids = None
                if os.name == "nt":
                    try:
                        file_ids = directory_file_ids(folder)
                    except OSError:
                        pass
                with os.scandir(folder) as entries:
                    for entry in entries:
                        if stopped(cancel):
                            return
                        raw_rel = (
                            f"{folder_rel}/{entry.name}" if folder_rel else entry.name
                        )
                        rel = normalized(raw_rel)
                        if entry.name.startswith(".myfotomanager") or any(
                            fnmatch.fnmatchcase(rel.casefold(), pattern.casefold())
                            for pattern in self.settings["exclusions"]
                        ):
                            continue
                        try:
                            if entry.is_symlink():
                                continue
                            if entry.is_dir(follow_symlinks=False):
                                stat = entry.stat(follow_symlinks=False)
                                if (
                                    entry.name.startswith(".")
                                    or entry.name.casefold() in SYSTEM_DIRS
                                    or getattr(stat, "st_file_attributes", 0) & 6
                                ):
                                    continue
                                stack.append((Path(entry.path), raw_rel))
                            elif (
                                entry.is_file(follow_symlinks=False)
                                and os.path.splitext(entry.name)[1][1:].lower()
                                in extensions
                            ):
                                stat = entry.stat(follow_symlinks=False)
                                if raw_rel != rel:
                                    self._physical[rel] = Path(entry.path)
                                file_id = stat.st_ino or (
                                    file_ids.get(entry.name, 0)
                                    if file_ids is not None
                                    else entry.inode()
                                )
                                yield (
                                    rel,
                                    stat,
                                    f"{device}:{file_id}" if file_id else None,
                                )
                        except OSError as exc:
                            result.errors.append(f"{rel}: {exc}")
            except OSError as exc:
                result.errors.append(f"{folder}: {exc}")

    def quick_scan(
        self, scope: str = "", progress=None, cancel=None, confirm_missing=None
    ) -> ScanResult:
        """Inventory files; incomplete scans never mark unseen entries missing.

        confirm_missing(count, total) must explicitly accept removals above 20%.
        Without a callback the scan preserves entries and reports needs_confirmation.
        """
        scope = self._scope(scope)
        result = self._begin("quick", scope)
        stats = result.stats
        stats.update(
            {
                key: 0
                for key in (
                    "added",
                    "modified",
                    "moved",
                    "missing",
                    "returned",
                    "count",
                    "uncertain",
                )
            }
        )
        stats.update(renamed_folders=[], ambiguous=[])
        start = time.monotonic()
        old = {
            row["relpath"]: dict(row) for row in self.db.execute("SELECT * FROM files")
        }
        folded = defaultdict(list)
        for row in old.values():
            folded[row["relpath_fold"]].append(row)
        seen, additions, updates, touches = set(), [], [], []
        try:
            for rel, stat, inode in self._walk(scope, result, cancel):
                stats["count"] += 1
                row = old.get(rel)
                if row is None and os.name == "nt" and len(folded[rel.casefold()]) == 1:
                    row = folded[rel.casefold()][0]
                if time.time_ns() - stat.st_mtime_ns < 10_000_000_000:
                    stats["uncertain"] += 1
                if row is None:
                    additions.append((rel, stat.st_size, stat.st_mtime_ns, inode))
                else:
                    seen.add(row["id"])
                    modified = (
                        row["size"] != stat.st_size
                        or abs(row["mtime_ns"] - stat.st_mtime_ns) > TOLERANCE_NS
                    )
                    if row["relpath"] != rel:
                        self._change(result.scan_id, "moved", row, row["relpath"], rel)
                        stats["moved"] += 1
                    if row["status"] == "missing":
                        self._change(result.scan_id, "returned", row, new=rel)
                        stats["returned"] += 1
                    if modified:
                        self._change(result.scan_id, "modified", row, new=rel)
                        stats["modified"] += 1
                    if (
                        not modified
                        and row["relpath"] == rel
                        and row["status"] == "present"
                    ):
                        touches.append(
                            (result.scan_id, stat.st_mtime_ns, inode, row["id"])
                        )
                    else:
                        parts = self._parts(rel)
                        updates.append(
                            (
                                *parts,
                                stat.st_size,
                                stat.st_mtime_ns,
                                inode,
                                result.scan_id,
                                int(modified),
                                int(modified),
                                int(modified),
                                row["id"],
                            )
                        )
                    if len(updates) + len(touches) >= 2000:
                        self._flush(updates, touches)
                if progress and stats["count"] % 2000 == 0:
                    progress(
                        dict(
                            phase="Schnellscan",
                            count=stats["count"],
                            rate=stats["count"] / max(time.monotonic() - start, 0.001),
                            remaining=None,
                        )
                    )
            self._flush(updates, touches)
            if stopped(cancel):
                # Discovered files are retained, but unseen files are untouched.
                self._insert_additions(additions, result)
                if additions:
                    self._pair(result.scan_id)
                return self._finish(result, "cancelled")
            if stats["count"] == 0:
                result.errors.append(
                    "Sammlung/Scope leer oder nicht erreichbar; keine Dateien als fehlend markiert."
                )
                return self._finish(result, "error")
            missing = [
                row
                for row in old.values()
                if row["id"] not in seen and in_scope(row["relpath"], scope)
            ]
            additions, missing = self._moves(additions, missing, result, cancel)
            self._insert_additions(additions, result)
            if additions:
                self._pair(result.scan_id)
            present_missing = [row for row in missing if row["status"] == "present"]
            total = sum(
                row["status"] == "present" and in_scope(row["relpath"], scope)
                for row in old.values()
            )
            if stopped(cancel):
                return self._finish(result, "cancelled")
            if result.errors:
                stats["missing_skipped"] = len(present_missing)
            elif (
                total
                and len(present_missing) / total > 0.2
                and not (
                    confirm_missing and confirm_missing(len(present_missing), total)
                )
            ):
                stats["missing_skipped"] = len(present_missing)
                return self._finish(result, "needs_confirmation")
            else:
                for row in present_missing:
                    self.db.execute(
                        "UPDATE files SET status='missing' WHERE id=?", (row["id"],)
                    )
                    self._change(result.scan_id, "missing", row, old=row["relpath"])
                stats["missing"] = len(present_missing)
            return self._finish(result)
        except BaseException as exc:
            self.db.rollback()
            result.errors.append(str(exc))
            self._finish(result, "error")
            raise

    @staticmethod
    def _parts(rel: str) -> tuple:
        directory, _, name = rel.rpartition("/")
        stem, extension = posixpath.splitext(name)
        return (
            rel,
            rel.casefold(),
            directory,
            name,
            stem,
            extension[1:].lower(),
        )

    def _flush(self, updates: list, touches: list | None = None) -> None:
        if touches:
            self.db.executemany(
                "UPDATE files SET last_seen_scan=?,mtime_ns=?,inode=? WHERE id=?",
                touches,
            )
            self.db.commit()
            touches.clear()
        if updates:
            self.db.executemany(
                """UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,
                size=?,mtime_ns=?,inode=?,last_seen_scan=?,status='present',
                quick_hash=CASE WHEN ? THEN NULL ELSE quick_hash END,
                full_hash=CASE WHEN ? THEN NULL ELSE full_hash END,
                meta_state=CASE WHEN ? THEN 'stale' ELSE meta_state END WHERE id=?""",
                updates,
            )
            self.db.commit()
            updates.clear()

    def _insert_additions(self, additions: list, result: ScanResult) -> None:
        for offset in range(0, len(additions), 2000):
            self.db.executemany(
                """INSERT INTO files(relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,inode,
                status,first_seen_scan,last_seen_scan) VALUES(?,?,?,?,?,?,?,?,?,'present',?,?)""",
                [
                    (
                        *self._parts(rel),
                        size,
                        mtime,
                        inode,
                        result.scan_id,
                        result.scan_id,
                    )
                    for rel, size, mtime, inode in additions[offset : offset + 2000]
                ],
            )
        result.stats["added"] = len(additions)

    def _hash(self, relpath: str, full: bool = False, cancel=None) -> str:
        path = self._path(relpath)
        if path.is_symlink() or not path.resolve().is_relative_to(self.root):
            raise ValueError("Dateipfad verlässt die Sammlung")
        return self._content_hash(path, full, cancel)

    @staticmethod
    def _content_hash(path: Path, full: bool = False, cancel=None) -> str:
        before = path.stat()
        digest = blake3()
        with path.open("rb") as stream:
            if full:
                while chunk := stream.read(1024 * 1024):
                    if stopped(cancel):
                        raise InterruptedError("Prüfung abgebrochen")
                    digest.update(chunk)
            else:
                digest.update(str(before.st_size).encode() + b"\0")
                digest.update(stream.read(65536))
                stream.seek(max(0, before.st_size - 65536))
                digest.update(stream.read(65536))
        after = path.stat()
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise OSError("Datei wurde während des Lesens geändert")
        return digest.hexdigest()

    def _moves(self, additions, missing, result, cancel):
        if not additions or not missing:
            return additions, missing
        remaining = {row["id"]: row for row in missing}
        new = {item[0]: item for item in additions}

        def move(item, row):
            rel, size, mtime, inode = item
            modified = (
                size != row["size"] or abs(mtime - row["mtime_ns"]) > TOLERANCE_NS
            )
            self.db.execute(
                """UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,
                size=?,mtime_ns=?,inode=?,status='present',last_seen_scan=?,
                meta_state=CASE WHEN ? THEN 'stale' ELSE meta_state END,
                quick_hash=CASE WHEN ? THEN NULL ELSE quick_hash END,
                full_hash=CASE WHEN ? THEN NULL ELSE full_hash END WHERE id=?""",
                (
                    *self._parts(rel),
                    size,
                    mtime,
                    inode,
                    result.scan_id,
                    modified,
                    modified,
                    modified,
                    row["id"],
                ),
            )
            self._change(result.scan_id, "moved", row, row["relpath"], rel)
            result.stats["moved"] += 1
            new.pop(rel)
            remaining.pop(row["id"])

        # Whole-folder signatures avoid content reads on the common rename path.
        old_dirs, new_dirs = defaultdict(list), defaultdict(list)
        for row in missing:
            old_dirs[row["dir"]].append(row)
        for item in additions:
            new_dirs[self._parts(item[0])[2]].append(item)
        signatures = defaultdict(list)
        for directory, items in new_dirs.items():
            signatures[
                tuple(sorted((PurePosixPath(item[0]).name, item[1]) for item in items))
            ].append(directory)
        old_signatures = defaultdict(list)
        for directory, rows in old_dirs.items():
            old_signatures[
                tuple(sorted((row["name"], row["size"]) for row in rows))
            ].append(directory)
        for signature, dirs in old_signatures.items():
            targets = signatures.get(signature, [])
            if len(dirs) == len(targets) == 1:
                source, target = dirs[0], targets[0]
                # Only optimize when the complete old directory disappeared.
                total = self.db.execute(
                    "SELECT COUNT(*) FROM files WHERE dir=? AND status='present'",
                    (source,),
                ).fetchone()[0]
                if total != len(old_dirs[source]):
                    continue
                by_name = {row["name"]: row for row in old_dirs[source]}
                for item in new_dirs[target]:
                    move(item, by_name[PurePosixPath(item[0]).name])
                result.stats["renamed_folders"].append({"old": source, "new": target})
        by_size = defaultdict(list)
        for row in remaining.values():
            by_size[row["size"]].append(row)
        # Resolve only mutually unique identities/hashes, never guess between copies.
        candidates = {}
        for rel, item in list(new.items()):
            if stopped(cancel):
                break
            rows = by_size[item[1]]
            inode_matches = [row for row in rows if item[3] and row["inode"] == item[3]]
            matches = inode_matches
            if not matches and rows:
                try:
                    digest = self._hash(rel)
                    matches = [
                        row
                        for row in rows
                        if row["quick_hash"] and row["quick_hash"] == digest
                    ]
                except OSError as exc:
                    result.errors.append(f"{rel}: {exc}")
            if matches:
                candidates[rel] = matches
        reverse = defaultdict(list)
        for rel, rows in candidates.items():
            for row in rows:
                reverse[row["id"]].append(rel)
        for rel, rows in candidates.items():
            if len(rows) == 1 and len(reverse[rows[0]["id"]]) == 1:
                move(new[rel], rows[0])
            else:
                result.stats["ambiguous"].append(
                    {"new": rel, "candidates": [row["relpath"] for row in rows]}
                )
        return list(new.values()), list(remaining.values())

    def _export(self, relpath: str) -> bool:
        folders = {part.casefold() for part in self.settings["export_folders"]}
        return any(part.casefold() in folders for part in relpath.split("/")[:-1])

    def _pair(self, scan_id: int) -> None:
        rows = [
            dict(row)
            for row in self.db.execute("SELECT * FROM files WHERE status='present'")
        ]
        groups = defaultdict(list)
        by_directory_name = defaultdict(list)
        for row in rows:
            if row["ext"] != "xmp":
                by_directory_name[(row["dir"], row["name"].casefold())].append(row)
        extensions = {value.casefold() for value in self.settings["extension_folders"]}
        for row in rows:
            directory = row["dir"]
            family = (
                directory.rpartition("/")[0]
                if directory.rpartition("/")[2].casefold() in extensions
                else directory
            )
            key = (family, row["stem"].casefold())
            if self._export(row["relpath"]):
                key = (row["relpath"], row["stem"])
            groups[key].append(row)
        sequence = self.db.execute(
            "SELECT seq FROM sqlite_sequence WHERE name='photos'"
        ).fetchone()
        next_photo = (sequence[0] if sequence else 0) + 1
        new_photos, assignments, sidecars = [], [], []
        for group in groups.values():
            media = [row for row in group if row["ext"] != "xmp"]
            for row in media:
                if row["photo_id"] is not None:
                    continue
                peers = [
                    other
                    for other in media
                    if other["photo_id"] is not None
                    and other["ext"] != row["ext"]
                    and abs(other["mtime_ns"] - row["mtime_ns"]) <= TOLERANCE_NS
                ]
                photo = min((other["photo_id"] for other in peers), default=None)
                if photo is None:
                    photo = next_photo
                    next_photo += 1
                    new_photos.append((photo, scan_id))
                row["photo_id"] = photo
                assignments.append((photo, row["id"]))
            for row in group:
                if row["ext"] == "xmp":
                    sidecars.append((row, media))
        for row, media in sidecars:
            # Resolve after all media groups, independent of directory order.
            matches = by_directory_name.get((row["dir"], row["stem"].casefold()), [])
            target = matches[0] if len(matches) == 1 else (media[0] if media else None)
            if target:
                assignments.append((target["photo_id"], row["id"]))
        self.db.executemany(
            "INSERT INTO photos(id,status,created_scan) VALUES(?,'present',?)",
            new_photos,
        )
        self.db.executemany("UPDATE files SET photo_id=? WHERE id=?", assignments)
        self.db.execute(
            """INSERT INTO changes(scan_id,kind,file_id,photo_id,new_relpath,at)
            SELECT ?, 'added', id, photo_id, relpath, ? FROM files WHERE first_seen_scan=?""",
            (scan_id, now(), scan_id),
        )

    def enrich_metadata(
        self, limit: int | None = None, progress=None, cancel=None
    ) -> ScanResult:
        """Resume pending/stale metadata in 200-file ExifTool batches."""
        result = self._begin("meta")
        rows = list(
            self.db.execute(
                """SELECT * FROM files WHERE status='present' AND meta_state IN ('pending','stale')
            ORDER BY ext IN ('mov','mp4'), dir DESC LIMIT ?""",
                (-1 if limit is None else limit,),
            )
        )
        result.stats.update(processed=0, errors_count=0)
        try:
            for offset in range(0, len(rows), 200):
                if stopped(cancel):
                    return self._finish(result, "cancelled")
                batch = rows[offset : offset + 200]
                raw_items = read_metadata(
                    [self._path(row["relpath"]) for row in batch],
                    self.settings["exiftool"],
                )
                by_path = {
                    os.path.normcase(str(Path(raw["SourceFile"]).resolve())): raw
                    for raw in raw_items
                }
                with self.db:
                    for row in batch:
                        if stopped(cancel):
                            break
                        try:
                            path = self._path(row["relpath"])
                            stat = path.stat()
                            if (stat.st_size, stat.st_mtime_ns) != (
                                row["size"],
                                row["mtime_ns"],
                            ):
                                raise OSError(
                                    "Datei seit Schnellscan geändert; erneut scannen"
                                )
                            raw = by_path.get(os.path.normcase(str(path.resolve())), {})
                            error = raw.get("Error") or raw.get("ExifTool:Error")
                            if not raw or error:
                                raise ValueError(error or "Keine Metadaten erhalten")
                            meta = metadata_values(raw)
                            columns = list(meta)
                            self.db.execute(
                                f"INSERT OR REPLACE INTO meta(file_id,{','.join(columns)}) VALUES({','.join('?' for _ in range(len(columns) + 1))})",
                                (row["id"], *meta.values()),
                            )
                            if row["ext"] != "xmp":
                                self._assign_identity(
                                    dict(row), identity_key(meta), result.scan_id
                                )
                            self.db.execute(
                                "UPDATE files SET meta_state='done',meta_error=NULL WHERE id=?",
                                (row["id"],),
                            )
                        except (OSError, ValueError) as exc:
                            self.db.execute(
                                "UPDATE files SET meta_state='error',meta_error=? WHERE id=?",
                                (str(exc), row["id"]),
                            )
                            result.errors.append(f"{row['relpath']}: {exc}")
                            result.stats["errors_count"] += 1
                        result.stats["processed"] += 1
                if progress:
                    progress(
                        dict(
                            phase="Metadaten",
                            count=result.stats["processed"],
                            total=len(rows),
                        )
                    )
            return self._finish(result, "cancelled" if stopped(cancel) else "done")
        except Exception as exc:
            result.errors.append(str(exc))
            return self._finish(result, "error")

    def _assign_identity(self, row: dict, key: str | None, scan_id: int) -> None:
        if not key or self._export(row["relpath"]):
            return
        current = row["photo_id"]
        if current is None:
            current = self.db.execute(
                "INSERT INTO photos(status,created_scan) VALUES('present',?)",
                (scan_id,),
            ).lastrowid
            self.db.execute(
                "UPDATE files SET photo_id=? WHERE id=?", (current, row["id"])
            )
        existing = self.db.execute(
            "SELECT id FROM photos WHERE identity_key=?", (key,)
        ).fetchone()
        current_key = self.db.execute(
            "SELECT identity_key FROM photos WHERE id=?", (current,)
        ).fetchone()[0]
        if current_key and current_key != key:
            # Weak stem pairing must not conflate distinct camera identities.
            photo = (
                existing[0]
                if existing
                else self.db.execute(
                    "INSERT INTO photos(identity_key,status,created_scan) VALUES(?,'present',?)",
                    (key, scan_id),
                ).lastrowid
            )
            self.db.execute(
                "UPDATE files SET photo_id=? WHERE id=?", (photo, row["id"])
            )
            return
        if existing and existing[0] != current:
            winner, loser = sorted((existing[0], current))
            self.db.execute("UPDATE photos SET identity_key=NULL WHERE id=?", (loser,))
            self.db.execute(
                "UPDATE files SET photo_id=? WHERE photo_id=?", (winner, loser)
            )
            # Keep the old anchor for future modules, with an explicit merge log.
            self.db.execute("UPDATE photos SET status='missing' WHERE id=?", (loser,))
            self.db.execute(
                "INSERT OR REPLACE INTO settings VALUES(?,?)",
                (f"photo.redirect.{loser}", json.dumps(winner)),
            )
            self._change(
                scan_id,
                "merged",
                dict(id=row["id"], photo_id=winner),
                str(loser),
                str(winner),
            )
            current = winner
        self.db.execute("UPDATE photos SET identity_key=? WHERE id=?", (key, current))

    def verify(self, scope: str = "", progress=None, cancel=None) -> ScanResult:
        """Hash complete files and report silent changes without replacing evidence."""
        scope = self._scope(scope)
        result = self._begin("verify", scope)
        result.stats.update(processed=0, mismatches=[], changed_since_scan=[])
        try:
            for row in self.db.execute(
                "SELECT * FROM files WHERE status='present'"
            ).fetchall():
                if not in_scope(row["relpath"], scope):
                    continue
                if stopped(cancel):
                    return self._finish(result, "cancelled")
                try:
                    stat = self._path(row["relpath"]).stat()
                    if (stat.st_size, stat.st_mtime_ns) != (
                        row["size"],
                        row["mtime_ns"],
                    ):
                        result.stats["changed_since_scan"].append(row["relpath"])
                        continue
                    digest = self._hash(row["relpath"], full=True, cancel=cancel)
                    if row["full_hash"] and row["full_hash"] != digest:
                        result.stats["mismatches"].append(row["relpath"])
                    else:
                        self.db.execute(
                            "UPDATE files SET full_hash=?,quick_hash=? WHERE id=?",
                            (digest, self._hash(row["relpath"]), row["id"]),
                        )
                    result.stats["processed"] += 1
                except OSError as exc:
                    result.errors.append(f"{row['relpath']}: {exc}")
                if progress:
                    progress(
                        dict(
                            phase="Vollständige Prüfung",
                            count=result.stats["processed"],
                        )
                    )
            return self._finish(result)
        except InterruptedError:
            return self._finish(result, "cancelled")

    def check_import(self, card_files: list[Path]) -> list[ImportStatus]:
        """Check identities first, then pending name/size candidates and hashes."""
        raw_items = []
        for offset in range(0, len(card_files), 200):
            raw_items.extend(
                read_metadata(
                    card_files[offset : offset + 200], self.settings["exiftool"]
                )
            )
        metadata = {
            os.path.normcase(str(Path(raw["SourceFile"]).resolve())): raw
            for raw in raw_items
        }
        results = []
        for path in card_files:
            raw = metadata.get(os.path.normcase(str(path.resolve())), {})
            if not raw or raw.get("Error") or raw.get("ExifTool:Error"):
                raise ValueError(f"Metadaten der Kartendatei nicht lesbar: {path}")
            key = identity_key(metadata_values(raw))
            photo = (
                self.db.execute(
                    "SELECT * FROM photos WHERE identity_key=?", (key,)
                ).fetchone()
                if key
                else None
            )
            if photo:
                file = self.db.execute(
                    "SELECT relpath FROM files WHERE photo_id=? ORDER BY status='present' DESC, id LIMIT 1",
                    (photo["id"],),
                ).fetchone()
                results.append(
                    ImportStatus(
                        path,
                        "imported"
                        if photo["status"] == "present"
                        else "imported_missing",
                        photo["id"],
                        file[0] if file else None,
                    )
                )
                continue
            # A known, different identity overrides filename collisions.
            candidates = self.db.execute(
                """SELECT f.* FROM files f JOIN photos p ON p.id=f.photo_id
                WHERE f.name=? AND f.size=? AND f.status='present' AND f.meta_state IN ('pending','stale','error')
                AND (? IS NULL OR p.identity_key IS NULL) ORDER BY f.id""",
                (normalized(path.name), path.stat().st_size, key),
            ).fetchall()
            candidate = None
            if candidates:
                card_hash = self._content_hash(path)
                for item in candidates:
                    collection_hash = self._hash(item["relpath"])
                    if not self.readonly:
                        self.db.execute(
                            "UPDATE files SET quick_hash=? WHERE id=?",
                            (collection_hash, item["id"]),
                        )
                    if collection_hash == card_hash:
                        candidate = item
                        break
            results.append(
                ImportStatus(
                    path,
                    "imported_likely" if candidate else "new",
                    candidate["photo_id"] if candidate else None,
                    candidate["relpath"] if candidate else None,
                )
            )
        if not self.readonly:
            self.db.commit()
        return results

    def changes_since(self, scan_id: int) -> Iterator[dict]:
        for row in self.db.execute(
            "SELECT * FROM changes WHERE scan_id>? ORDER BY id", (scan_id,)
        ):
            yield dict(row)

    def photo_files(self, photo_id: int) -> list[dict]:
        return [
            dict(row)
            for row in self.db.execute(
                "SELECT * FROM files WHERE photo_id=? ORDER BY id", (photo_id,)
            )
        ]

    def status(self) -> dict:
        counts = dict(
            self.db.execute(
                "SELECT status,COUNT(*) FROM files GROUP BY status"
            ).fetchall()
        )
        return dict(
            files=counts.get("present", 0),
            missing=counts.get("missing", 0),
            photos=self.db.execute(
                "SELECT COUNT(*) FROM photos WHERE status='present'"
            ).fetchone()[0],
            metadata=self.db.execute(
                "SELECT COUNT(*) FROM files WHERE status='present' AND meta_state='done'"
            ).fetchone()[0],
            last_scan=self.db.execute(
                "SELECT MAX(finished_at) FROM scans WHERE kind='quick' AND status='done'"
            ).fetchone()[0],
            collection_id=self.db.execute("SELECT id FROM collection").fetchone()[0],
            readonly=self.readonly,
        )

    def purge_missing(self, older_than_scans: int = 0) -> int:
        """Explicitly purge missing file records; preserve photo anchors/user data."""
        self._writable()
        latest = self.db.execute("SELECT COALESCE(MAX(id),0) FROM scans").fetchone()[0]
        with self.db:
            ids = [
                row[0]
                for row in self.db.execute(
                    "SELECT id FROM files WHERE status='missing' AND last_seen_scan<=?",
                    (latest - older_than_scans,),
                )
            ]
            self.db.executemany(
                "DELETE FROM meta WHERE file_id=?", [(value,) for value in ids]
            )
            self.db.executemany(
                "DELETE FROM files WHERE id=?", [(value,) for value in ids]
            )
        self.backup()
        return len(ids)

    def resolve_move(self, old_relpath: str, new_relpath: str) -> None:
        """Apply an explicitly reviewed ambiguous move while retaining anchors."""
        self._writable()
        old = self.db.execute(
            "SELECT * FROM files WHERE relpath=?", (normalized(old_relpath),)
        ).fetchone()
        new = self.db.execute(
            "SELECT * FROM files WHERE relpath=? AND status='present'",
            (normalized(new_relpath),),
        ).fetchone()
        if (
            not old
            or not new
            or old["id"] == new["id"]
            or self._path(old["relpath"]).exists()
        ):
            raise ValueError(
                "Bisheriger Pfad muss fehlen und neuer Pfad vorhanden sein"
            )
        if (
            old["size"] != new["size"]
            or not old["quick_hash"]
            or old["quick_hash"] != self._hash(new["relpath"])
        ):
            raise ValueError("Inhaltsfingerabdrücke stimmen nicht überein")
        result = self._begin("quick")
        with self.db:
            self.db.execute("DELETE FROM meta WHERE file_id=?", (new["id"],))
            self.db.execute("DELETE FROM files WHERE id=?", (new["id"],))
            self.db.execute(
                """UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,
                size=?,mtime_ns=?,inode=?,last_seen_scan=?,status='present' WHERE id=?""",
                (
                    *self._parts(new["relpath"]),
                    new["size"],
                    new["mtime_ns"],
                    new["inode"],
                    result.scan_id,
                    old["id"],
                ),
            )
            self._change(result.scan_id, "moved", old, old["relpath"], new["relpath"])
        self._finish(result)
