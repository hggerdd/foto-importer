"""Own face workers and marshal data back to Tk through UiDispatcher."""

from __future__ import annotations

import sqlite3
import threading
from collections import OrderedDict
from pathlib import Path

from config.settings import FACE_MODEL_DIR
from core.collection_index import CollectionIndex
from core.face_models import ModelRegistry
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

    def _status(self, message: str) -> None:
        if self.view and self.view.winfo_exists():
            self.view.status.set(message)

    def run(
        self, action=None, *, root: Path | None = None, scope: str = "", refresh=True
    ) -> None:
        if self.closed:
            return
        if self.busy:
            if action is None:
                self._refresh_pending = True
            else:
                self._status(
                    "Gesichtsauftrag läuft. Bitte Abschluss abwarten oder abbrechen."
                )
            return
        root = root or self.target()
        if not root or not (root / ".myfotomanager").exists():
            self._status("Bitte zuerst die Sammlung im Index erfassen.")
            return
        self.busy = True
        self.cancel.clear()
        revision = self.revision
        mode = self.view.mode.get() if self.view else "Unbekannte"
        person = self.view.selected_person() if self.view else None
        page = self.view.page if self.view else 0
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
                            self._snapshot(service, mode, person, page)
                            if refresh
                            else None
                        )
                self.dispatcher.post(self._done, revision, root, snapshot, result)
            except Exception as exc:  # noqa: BLE001 - report worker/runtime failures on the UI thread
                self.dispatcher.post(self._error, revision, str(exc))

        threading.Thread(target=worker, name="FaceService", daemon=True).start()

    def _snapshot(self, service, mode, person, page) -> dict:
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
        page = min(page, max(0, (total - 1) // 60))
        rows = rows[page * 60 : (page + 1) * 60]
        for row in rows:
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
                    self._cache[key] = service.thumbnail(row["id"], 112)
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
            "dataset": dataset,
            "relations": service.graph.derived(active) if active else {},
            "photos": service.person_photos(active) if active else [],
        }

    def _done(self, revision, root, snapshot, result) -> None:
        self.busy = False
        if self.closed:
            return
        if revision != self.revision or root != self.target():
            self.run()
            return
        if self.view and self.view.winfo_exists():
            if snapshot:
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
        elif not self.closed:
            self.run()

    def target_changed(self) -> None:
        self.revision += 1
        self.cancel.set()
        self._cache.clear()
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
