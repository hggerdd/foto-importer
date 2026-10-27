"""Store basic relations only; derive family relationships on demand."""
from __future__ import annotations

import sqlite3
import warnings


class PersonGraph:
    def __init__(self, db: sqlite3.Connection) -> None:
        self.db = db

    def _person(self, person: int) -> None:
        if not self.db.execute("SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL", (person,)).fetchone():
            raise ValueError("Person fehlt oder wurde zusammengeführt")

    def edges(self) -> set[tuple[int, int]]:
        return {tuple(r) for r in self.db.execute("SELECT from_person,to_person FROM relations WHERE type_id=1")}

    def add(self, source: int, kind: str, target: int) -> None:
        """Validate and insert; inverse and symmetric relations use canonical storage."""
        self._person(source)
        self._person(target)
        if source == target:
            raise ValueError("Keine Selbstbeziehung erlaubt")
        relation = self.db.execute("SELECT * FROM relation_types WHERE name=?", (kind,)).fetchone()
        if not relation:
            raise ValueError("Unbekannter Beziehungstyp")
        type_id = relation["id"]
        if type_id == 2:
            source, target, type_id = target, source, 1
        if type_id in (3, 4):
            source, target = sorted((source, target))
        if type_id == 1:
            edges = self.edges() | {(source, target)}
            self.validate(edges)
            if len({a for a, b in edges if b == target}) > 2:
                warnings.warn("Mehr als zwei Elternteile eingetragen", UserWarning, stacklevel=2)
        self.db.execute("INSERT OR IGNORE INTO relations VALUES(?,?,?)", (source, type_id, target))

    @staticmethod
    def validate(edges: set[tuple[int, int]]) -> None:
        nodes = {n for edge in edges for n in edge}
        visiting, visited = set(), set()

        def visit(node):
            if node in visiting:
                raise ValueError("Zyklus in Elternbeziehungen")
            if node in visited:
                return
            visiting.add(node)
            for a, b in edges:
                if a == node:
                    visit(b)
            visiting.remove(node)
            visited.add(node)

        for node in nodes:
            visit(node)

    def remove(self, source: int, kind: str, target: int) -> None:
        type_id = self.db.execute("SELECT id FROM relation_types WHERE name=?", (kind,)).fetchone()[0]
        if type_id == 2:
            source, target, type_id = target, source, 1
        if type_id in (3, 4):
            source, target = sorted((source, target))
        self.db.execute("DELETE FROM relations WHERE from_person=? AND type_id=? AND to_person=?", (source, type_id, target))

    def derived(self, person: int) -> dict[str, list[int]]:
        edges = self.edges()
        parents = {a for a, b in edges if b == person}
        children = {b for a, b in edges if a == person}
        siblings = {b for a, b in edges if a in parents and b != person}
        for a, b in self.db.execute("SELECT from_person,to_person FROM relations WHERE type_id=4"):
            if person in (a, b):
                siblings.add(b if a == person else a)
        grandparents = {a for a, b in edges if b in parents}
        uncles = set()
        for parent in parents:
            parent_parents = {a for a, b in edges if b == parent}
            uncles |= {b for a, b in edges if a in parent_parents} - parents
            for a, b in self.db.execute("SELECT from_person,to_person FROM relations WHERE type_id=4"):
                if parent in (a, b):
                    uncles.add(b if a == parent else a)
        return {key: sorted(value - {person}) for key, value in {
            "eltern": parents, "kinder": children, "geschwister": siblings,
            "grosseltern": grandparents, "enkel": {b for a, b in edges if a in children},
            "onkel_tanten": uncles, "cousins": {b for a, b in edges if a in uncles},
            "partner": {b if a == person else a for a, b in self.db.execute(
                "SELECT from_person,to_person FROM relations WHERE type_id=3") if person in (a, b)},
        }.items()}
