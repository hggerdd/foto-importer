"""Own face workers and marshal data back to Tk through UiDispatcher."""

from __future__ import annotations

import sqlite3
import threading
from collections import OrderedDict, deque
from pathlib import Path

from config.settings import (
    FACE_GROUP_PREVIEWS,
    FACE_GROUP_THUMBNAIL_SIZE,
    FACE_GROUPS_PER_PAGE,
    FACE_MODEL_DIR,
    FACE_NAMING_UNDO_LIMIT,
)
from core.collection_index import CollectionIndex
from core.face_models import ModelRegistry
from core.face_naming import assign_name, undo_name
from core.face_service import FaceService
from core.face_store import settings
from core.task_scheduler import background_thread
from ui.dispatcher import UiDispatcher


class FaceController:
    def __init__(self, root, target, scheduler=None) -> None:
        self.target, self.scheduler = target, scheduler
        self.dispatcher = UiDispatcher(root)
        self.cancel = threading.Event()
        self.view = None
        self.busy = False
        self.closed = False
        self.revision = 0
        self.registry = None
        self._refresh_pending = False
        self._cache = OrderedDict()
        self._undo = deque(maxlen=FACE_NAMING_UNDO_LIMIT)

    @property
    def can_undo(self) -> bool:
        return bool(self._undo) and not self.busy

    def assign_group(
        self,
        face_ids: list[int],
        name: str,
        person_id: int | None = None,
        excluded_ids: list[int] | None = None,
    ) -> bool:
        def operation(service):
            change = assign_name(service, face_ids, name, person_id, excluded_ids)
            return {
                "naming_change": change,
                "named_count": len(face_ids),
                "name": change.name,
            }

        return self.run(operation, naming=True)

    def undo_naming(self) -> None:
        if not self._undo or self.busy:
            return
        change = self._undo[-1]

        def operation(service):
            undo_name(service, change)
            return {"undone": change}

        self.run(operation, naming=True)

    def _status(self, message: str) -> None:
        if self.view and self.view.winfo_exists():
            self.view.status.set(message)

    def run(
        self,
        action=None,
        *,
        root: Path | None = None,
        scope: str = "",
        refresh=True,
        naming=False,
    ) -> bool:
        if self.closed:
            return False
        if self.busy:
            if action is None:
                self._refresh_pending = True
            else:
                self._status(
                    "Gesichtsauftrag läuft. Bitte Abschluss abwarten oder abbrechen."
                )
            return False
        root = root or self.target()
        if not root or not (root / ".myfotomanager").exists():
            self._status("Bitte zuerst die Sammlung im Index erfassen.")
            return False
        self.busy = True
        self.cancel.clear()
        revision = self.revision
        view = self.view if self.view and self.view.winfo_exists() else None
        mode = view.mode.get() if view else "Unbekannte"
        person = view.selected_person() if view else None
        page = view.page if view else 0
        offsets = dict(view.preview_offsets) if view else {}
        if action is not None and not naming:
            self._undo.clear()
        self._status("Gesichtsdaten werden geladen …")

        def worker():
            try:
                with background_thread():
                    if self.registry is None:
                        self.registry = ModelRegistry(FACE_MODEL_DIR)
                    with CollectionIndex.open(root) as index:
                        index._writable()
                        service = FaceService(index, self.registry)
                        result = None
                        if action == "scan":
                            result = service.scan(
                                scope,
                                self.cancel,
                                lambda value: self.dispatcher.post(
                                    self._status,
                                    f"Analyse {value['done'] + value['skipped']}/{value['total']} · {len(value['errors'])} Fehler",
                                    key="face-progress",
                                ),
                                scheduler=self.scheduler,
                            )
                        elif callable(action):
                            result = action(service)
                        snapshot = (
                            self._snapshot(service, mode, person, page, offsets)
                            if refresh
                            else None
                        )
                self.dispatcher.post(self._done, revision, root, snapshot, result)
            except Exception as exc:  # noqa: BLE001 - report worker/runtime failures on the UI thread
                self.dispatcher.post(self._error, revision, str(exc))

        threading.Thread(target=worker, name="FaceService", daemon=True).start()
        return True

    def _snapshot(self, service, mode, person, page, offsets=None) -> dict:
        offsets = offsets or {}
        groups = []
        if mode == "Unbekannte":
            groups = service.clusters()
            ids = [face for cluster in groups for face in cluster]
            rows_by_id = {
                r["id"]: r
                for r in service.face_rows(
                    "ignored=0 AND person_id IS NULL AND retained=0"
                )
            }
            rows = [rows_by_id[i] for i in ids if i in rows_by_id]
        elif mode == "Vorschläge":
            rows = service.face_rows(
                "assignment IN ('suggested','auto') AND ignored=0 AND retained=0"
            )
            by_person = {}
            for row in rows:
                by_person.setdefault(row["person_id"], []).append(row["id"])
            groups = list(by_person.values())
        elif mode == "Bitte prüfen":
            rows = service.face_rows(
                "f.id IN (SELECT face_id FROM face_review WHERE embedder_id=?)",
                (service.embedder_key,),
            )
        elif mode == "Alle Gesichter":
            rows = service.face_rows("retained=0 OR assignment='user'")
        elif mode in ("Referenz", "Validierung"):
            rows = service.face_rows(
                "f.person_id=? AND f.id IN (SELECT face_id FROM training_set WHERE embedder_id=? AND split=?)",
                (
                    person or -1,
                    service.embedder_key,
                    "ref" if mode == "Referenz" else "val",
                ),
            )
        else:
            rows = service.face_rows("person_id=? AND ignored=0", (person or -1,))
        total = len(rows)
        grouped = mode in ("Unbekannte", "Vorschläge")
        if grouped:
            pages = max(
                1, (len(groups) + FACE_GROUPS_PER_PAGE - 1) // FACE_GROUPS_PER_PAGE
            )
            page = min(page, pages - 1)
            groups = groups[
                page * FACE_GROUPS_PER_PAGE : (page + 1) * FACE_GROUPS_PER_PAGE
            ]
            ids = {face for group in groups for face in group}
            rows = [row for row in rows if row["id"] in ids]
            preview_ids = set()
            for group in groups:
                start = max(0, min(offsets.get(group[0], 0), max(0, len(group) - 1)))
                preview_ids.update(group[start : start + FACE_GROUP_PREVIEWS])
        else:
            pages = max(1, (total + 59) // 60)
            page = min(page, pages - 1)
            rows = rows[page * 60 : (page + 1) * 60]
            preview_ids = {row["id"] for row in rows}
        for row in rows:
            if row["id"] not in preview_ids:
                continue
            if self.cancel.is_set():
                break
            try:
                path = service.index.root / row["relpath"]
                stat = path.stat()
                key = (
                    service.index.root,
                    row["id"],
                    service._bbox(row),
                    row["orientation"],
                    stat.st_mtime_ns,
                    stat.st_ctime_ns,
                )
                if key not in self._cache:
                    self._cache[key] = service.thumbnail(
                        row["id"], FACE_GROUP_THUMBNAIL_SIZE
                    )
                    while len(self._cache) > 256:
                        self._cache.popitem(last=False)
                self._cache.move_to_end(key)
                row["image"] = self._cache[key]
            except Exception as exc:  # noqa: BLE001 - one failed thumbnail must not hide other faces
                row["image_error"] = str(exc)
        persons = service.persons()
        active = person if any(p["id"] == person for p in persons) else None
        dataset = [
            dict(r)
            for r in service.db.execute(
                "SELECT t.*,f.captured_at FROM training_set t JOIN faces f ON f.id=t.face_id WHERE t.person_id=? AND t.embedder_id=?",
                (active, service.embedder_key),
            )
        ]
        return {
            "rows": rows,
            "groups": groups,
            "persons": persons,
            "models": service.registry.list_models(),
            "model_errors": service.registry.errors,
            "settings": service.options,
            "status": service.status(),
            "page": page,
            "total": total,
            "pages": pages,
            "mode": mode,
            "group_start": page * FACE_GROUPS_PER_PAGE,
            "named_faces": service.db.execute(
                "SELECT count(*) FROM faces WHERE person_id IS NOT NULL AND ignored=0 AND (retained=0 OR assignment='user')"
            ).fetchone()[0],
            "dataset": dataset,
            "relations": service.graph.derived(active) if active else {},
            "photos": service.person_photos(active) if active else [],
        }

    def _done(self, revision, root, snapshot, result) -> None:
        self.busy = False
        if self.closed:
            return
        if revision != self.revision or root != self.target():
            self._undo.clear()
            self.run()
            return
        if isinstance(result, dict) and "naming_change" in result:
            self._undo.append(result["naming_change"])
        elif isinstance(result, dict) and "undone" in result:
            self._undo.pop()
            if self.view and self.view.winfo_exists():
                self.view._focus_after_undo = {
                    face["id"] for face in result["undone"].faces
                }
        if self.view and self.view.winfo_exists():
            if snapshot and snapshot.get("mode") != self.view.mode.get():
                self._refresh_pending = True
            elif snapshot:
                self.view.render(snapshot)
                if snapshot["status"]["faces"] == 0:
                    self._cache.clear()
            if result is not None:
                self.view.show_result(result)
        if self._refresh_pending:
            self._refresh_pending = False
            self.run()

    def _error(self, revision, error) -> None:
        self.busy = False
        if revision == self.revision:
            self._status(error)
            if self.view and self.view.winfo_exists():
                self.view._pending_naming = False
                self.view._focus_after_assign = None
                for row in self.view.group_rows:
                    row._selection_changed()
        elif not self.closed:
            self.run()

    def target_changed(self) -> None:
        self.revision += 1
        self.cancel.set()
        self._cache.clear()
        self._undo.clear()
        if self.view and self.view.winfo_exists():
            self.view.clear()
        if not self.busy and self.view and self.view.winfo_exists():
            self.run()

    def automatic(self, root: Path, scope: str = "", *, nightly=False) -> None:
        if self.closed or self.busy:
            return

        def enabled():
            try:
                with CollectionIndex.open(root, readonly=True) as index:
                    if index.db.execute("PRAGMA user_version").fetchone()[0] < 2:
                        return
                    enabled = settings(index.db).get(
                        "nightly" if nightly else "auto_import", False
                    )
                if enabled:
                    self.dispatcher.post(self.run_auto, root, scope)
            except (OSError, ValueError, sqlite3.Error):
                return

        threading.Thread(target=enabled, name="FaceAutoCheck", daemon=True).start()

    def run_auto(self, root, scope) -> None:
        if root == self.target():
            self.run("scan", root=root, scope=scope, refresh=self.view is not None)

    def close(self) -> None:
        self.closed = True
        self.cancel.set()
        self.dispatcher.close()
