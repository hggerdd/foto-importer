"""Face review workspace: models, clusters, suggestions, people and references."""

from __future__ import annotations

import json
import tkinter as tk
import warnings
from pathlib import Path
from tkinter import messagebox, simpledialog, ttk

import numpy as np
from PIL import ImageTk

from core.face_dataset import calibrate, export_dataset
from ui.scroll_frame import ScrollFrame


class FaceView(ttk.Frame):
    def __init__(self, parent, controller) -> None:
        super().__init__(parent, padding=12)
        self.controller = controller
        controller.view = self
        self.page = 0
        self.snapshot = {}
        self.images = []
        self.selected = {}
        self.cards = []
        self.status = tk.StringVar(
            value="Lokal auf der CPU · Modelle separat installieren (models/README.md)."
        )
        self.mode = tk.StringVar(value="Unbekannte")
        self.person = tk.StringVar()
        self.people = {}
        top = ttk.Frame(self)
        top.pack(fill="x")
        for text, command in (
            ("Sammlung analysieren", lambda: controller.run("scan")),
            ("Ordner analysieren …", self.scan_folder),
            ("Abbrechen", controller.cancel.set),
            ("Aktualisieren", self.reload),
            ("Modelle / Einstellungen …", self.configure_models),
        ):
            ttk.Button(top, text=text, command=command).pack(side="left", padx=3)
        ttk.Label(self, textvariable=self.status, wraplength=1250).pack(
            fill="x", pady=8
        )
        filters = ttk.Frame(self)
        filters.pack(fill="x")
        combo = ttk.Combobox(
            filters,
            textvariable=self.mode,
            state="readonly",
            width=18,
            values=(
                "Unbekannte",
                "Vorschläge",
                "Person",
                "Referenz",
                "Validierung",
                "Bitte prüfen",
                "Alle Gesichter",
            ),
        )
        combo.pack(side="left")
        combo.bind("<<ComboboxSelected>>", lambda _: self.reload(reset=True))
        self.person_combo = ttk.Combobox(filters, textvariable=self.person, width=30)
        self.person_combo.pack(side="left", padx=6)
        self.person_combo.bind(
            "<<ComboboxSelected>>", lambda _: self.reload(reset=True)
        )
        self.person_combo.bind("<KeyRelease>", self.autocomplete)
        ttk.Button(filters, text="Neue Person …", command=self.new_person).pack(
            side="left"
        )
        ttk.Button(
            filters, text="Person / Beziehungen …", command=self.person_details
        ).pack(side="left", padx=6)
        ttk.Button(filters, text="←", command=lambda: self.turn_page(-1)).pack(
            side="right"
        )
        ttk.Button(filters, text="→", command=lambda: self.turn_page(1)).pack(
            side="right"
        )
        self.surface = ScrollFrame(self)
        self.surface.pack(fill="both", expand=True, pady=10)
        self.surface.canvas.bind("<Configure>", self._reflow, add="+")
        actions = ttk.Frame(self)
        actions.pack(fill="x")
        for number, (text, command) in enumerate(
            (
                ("Auswahl zuordnen", self.assign),
                ("Ja / bestätigen (J)", lambda: self.face_action("confirm")),
                ("Nein / ablehnen (N)", lambda: self.face_action("reject")),
                ("Überspringen (S)", self.skip),
                ("Ignorieren", lambda: self.face_action("ignore")),
                ("Fixieren", lambda: self.face_action("pin")),
                ("Fixierung lösen", lambda: self.face_action("unpin")),
                (
                    "Aus Datensatz entfernen",
                    lambda: self.face_action("exclude_training"),
                ),
            )
        ):
            ttk.Button(actions, text=text, command=command).grid(
                row=number // 4, column=number % 4, sticky="ew", padx=2, pady=2
            )
        bottom = ttk.Frame(self)
        bottom.pack(fill="x", pady=(8, 0))
        for text, command in (
            (
                "Datensatz neu aufbauen",
                lambda: controller.run(
                    lambda s: (s.rebuild_training(), s.rematch())[0]
                ),
            ),
            ("Datensatz exportieren", lambda: controller.run(export_dataset)),
            ("Kalibrierung …", lambda: controller.run(calibrate)),
            ("Alle Gesichtsdaten löschen …", self.delete_all),
        ):
            ttk.Button(bottom, text=text, command=command).pack(side="left", padx=3)
        for key, command in (
            ("j", lambda: self.face_action("confirm")),
            ("n", lambda: self.face_action("reject")),
            ("s", self.skip),
        ):
            self.bind(key, lambda event, callback=command: callback())

    def selected_person(self) -> int | None:
        return self.people.get(self.person.get())

    def autocomplete(self, event=None) -> None:
        query = self.person.get().casefold()
        self.person_combo.configure(
            values=[name for name in self.people if query in name.casefold()]
        )

    def clear(self) -> None:
        for child in self.surface.content.winfo_children():
            child.destroy()
        self.images.clear()
        self.cards.clear()
        self.selected.clear()
        self.snapshot = {}
        self.people = {}
        self.person.set("")
        self.page = 0

    def reload(self, reset=False) -> None:
        if reset:
            self.page = 0
        self.controller.run()

    def render(self, snapshot) -> None:
        self.snapshot = snapshot
        self.page = snapshot["page"]
        self.people = {
            f"{p['attrs'].get('name', 'Unbenannt')} · #{p['id']}": p["id"]
            for p in snapshot["persons"]
        }
        self.person_combo.configure(values=list(self.people))
        for child in self.surface.content.winfo_children():
            child.destroy()
        self.images, self.selected = [], {}
        self.cards = []
        self._group_selection = []
        cluster_map = {
            face: i + 1 for i, group in enumerate(snapshot["groups"]) for face in group
        }
        people = {
            p["id"]: p["attrs"].get("name", f"#{p['id']}") for p in snapshot["persons"]
        }
        pins = {r["face_id"] for r in snapshot["dataset"] if r["pinned"]}
        for position, face in enumerate(snapshot["rows"]):
            frame = ttk.Frame(self.surface.content, padding=8, relief="solid")
            self.cards.append(frame)
            frame.grid(
                row=position // 6, column=position % 6, sticky="nsew", padx=3, pady=3
            )
            variable = tk.BooleanVar()
            self.selected[face["id"]] = variable
            if face.get("image"):
                photo = ImageTk.PhotoImage(face["image"], master=self)
                self.images.append(photo)
                label = ttk.Label(frame, image=photo)
            else:
                label = ttk.Label(frame, text="Vorschau fehlt", width=16)
            label.pack()
            label.bind(
                "<Button-1>",
                lambda _, v=variable: (v.set(not v.get()), self.focus_set()),
            )
            group = cluster_map.get(face["id"])
            title = (
                f"Gruppe {group}"
                if group
                else people.get(face["person_id"], "Unbekannt")
            )
            ttk.Checkbutton(
                frame, text=f"{title} · #{face['id']}", variable=variable
            ).pack(anchor="w")
            info = f"Qualität {face['quality']:.2f}"
            if face["assignment"]:
                info += f" · {face['assignment']}"
            if face["retained"]:
                info += " · beibehalten"
            if face["id"] in pins:
                info += " · fixiert"
            ttk.Label(frame, text=info, wraplength=170).pack(anchor="w")
            ttk.Label(frame, text=face["relpath"], wraplength=170).pack(anchor="w")
            if group:
                ttk.Button(
                    frame,
                    text="Gruppe auswählen",
                    command=lambda g=group: self.select_group(g, cluster_map),
                ).pack()
        stats = snapshot["status"]
        self.status.set(
            f"{stats['faces']} Gesichter · {stats['persons']} Personen · {snapshot['total']} Treffer · Seite {self.page + 1} · "
            "Gesicht anklicken, Gruppe auswählen oder Vorschläge mit J / N / S prüfen."
        )
        self.surface.bind_wheel()
        self._reflow()

    def _reflow(self, event=None) -> None:
        columns = max(1, min(6, self.surface.canvas.winfo_width() // 200))
        for position, card in enumerate(self.cards):
            card.grid_configure(row=position // columns, column=position % columns)

    def select_group(self, group, mapping) -> None:
        # Include the complete group even when its members cross a page boundary.
        self._group_selection = [
            face
            for face, value in mapping.items()
            if value == group and face not in self.selected
        ]
        for face, variable in self.selected.items():
            variable.set(mapping.get(face) == group)
        self.focus_set()

    def ids(self) -> list[int]:
        visible = [face for face, variable in self.selected.items() if variable.get()]
        group = getattr(self, "_group_selection", [])
        return sorted(set(group + visible))

    def assign(self) -> None:
        ids, person = self.ids(), self.selected_person()
        if ids and person:
            self.controller.run(lambda s: s.assign(ids, person))
        else:
            self.status.set(
                "Bitte Gesichter und eine bestehende Person auswählen oder eine Person anlegen."
            )

    def face_action(self, action) -> None:
        ids = self.ids() or [r["id"] for r in self.snapshot.get("rows", [])[:1]]
        if not ids:
            return

        def operation(service):
            for face in ids:
                if action == "unpin":
                    service.pin(face, False)
                else:
                    getattr(service, action)(face)

        self.controller.run(operation)

    def skip(self) -> None:
        rows = self.snapshot.get("rows", [])
        if rows:
            self.snapshot = self.snapshot | {"rows": rows[1:] + rows[:1]}
            self.render(self.snapshot)
        self.focus_set()

    def turn_page(self, delta) -> None:
        self.page = max(0, self.page + delta)
        self.reload()

    def new_person(self) -> None:
        name = simpledialog.askstring(
            "Person anlegen",
            "Name (optional, Identität ist die Personen-ID):",
            parent=self,
        )
        if name is not None:
            self.controller.run(
                lambda s: {"person_id": s.create_person({"name": name})}
            )

    def scan_folder(self) -> None:
        from tkinter import filedialog

        root = self.controller.target()
        if not root:
            return
        folder = filedialog.askdirectory(parent=self, initialdir=root)
        if folder:
            try:
                scope = Path(folder).resolve().relative_to(root.resolve()).as_posix()
            except ValueError:
                self.status.set("Ordner muss innerhalb der Sammlung liegen.")
                return
            self.controller.run("scan", scope="" if scope == "." else scope)

    def configure_models(self) -> None:
        snapshot = self.snapshot
        if not snapshot:
            self.reload()
            return
        dialog = tk.Toplevel(self)
        dialog.title("Lokale Modelle und Datensatz")
        surface = ScrollFrame(dialog)
        surface.pack(fill="both", expand=True)
        pane = surface.content
        dialog.geometry("900x800")
        options, variables = snapshot["settings"], {}
        ttk.Label(
            pane,
            text="Detektorwechsel (fortgeschritten): Sicherung wird angelegt; anschließend analysieren.\n"
            "Embedderwechsel: anschließend analysieren, um fehlende Embeddings aufzubauen.\n"
            "Installation: uv run python scripts/install_face_models.py [Modell-IDs]",
            wraplength=830,
        ).pack(anchor="w", pady=8)
        for model in snapshot["models"]:
            size = (
                f"{model['size'] / 1024**2:.1f} MB"
                if model.get("size")
                else "Größe unbekannt"
            )
            ttk.Label(
                pane,
                text=f"{model['id']} · {'installiert' if model['installed'] else 'fehlt'} · {size} · CPU: {model['speed']}\n"
                f"{model['license']}\n{model['source_url']}",
                wraplength=830,
            ).pack(anchor="w", pady=4)
        for key, title, kind in (
            ("active_detector", "Detektor", "detector"),
            ("active_embedder", "Embedder", "embedder"),
        ):
            ttk.Label(pane, text=title).pack(anchor="w")
            variables[key] = tk.StringVar(value=options[key])
            ttk.Combobox(
                pane,
                textvariable=variables[key],
                state="readonly",
                values=[m["id"] for m in snapshot["models"] if m["kind"] == kind],
            ).pack(fill="x")
        active = next(
            m for m in snapshot["models"] if m["id"] == options["active_embedder"]
        )
        for key, title, default in (
            ("auto", "Schwelle automatisch", active["thresholds"]["auto"]),
            ("suggest", "Schwelle Vorschlag", active["thresholds"]["suggest"]),
            (
                "duplicate",
                "Duplikat-Schwelle",
                active["thresholds"].get("duplicate", 0.92),
            ),
            ("min_quality", "Mindestqualität", 0.5),
            ("min_score", "Mindestscore", 0.8),
            ("reference_limit", "Max. Gesichter pro Person", 100),
            ("validation_fraction", "Validierungsanteil", 0.2),
        ):
            row = ttk.Frame(pane)
            row.pack(fill="x", pady=3)
            ttk.Label(row, text=title, width=30).pack(side="left")
            variables[key] = tk.StringVar(value=str(options.get(key, default)))
            ttk.Entry(row, textvariable=variables[key]).pack(side="left")
        for key, title in (
            ("auto_import", "Neue Importe automatisch analysieren"),
            (
                "nightly",
                "Nachts um 03:00 analysieren / Datensatz aufbauen (App muss laufen)",
            ),
            ("age_prior", "Geburtsjahr berücksichtigen"),
            (
                "cooccurrence",
                "Gemeinsame Fotos bei unsicheren Vorschlägen berücksichtigen",
            ),
        ):
            variables[key] = tk.BooleanVar(
                value=options.get(key, key in ("age_prior", "cooccurrence"))
            )
            ttk.Checkbutton(pane, text=title, variable=variables[key]).pack(anchor="w")

        def save():
            try:
                values = {key: value.get() for key, value in variables.items()}
                for key in (
                    "auto",
                    "suggest",
                    "duplicate",
                    "min_quality",
                    "min_score",
                    "validation_fraction",
                ):
                    values[key] = float(values[key])
                values["reference_limit"] = int(values["reference_limit"])
                if values["active_embedder"] != options["active_embedder"]:
                    # Switching models uses that model's thresholds on first save.
                    for key in ("auto", "suggest", "duplicate"):
                        values.pop(key)
                self.controller.run(lambda s: s.configure(**values))
                dialog.destroy()
            except ValueError as exc:
                messagebox.showerror("Einstellungen", str(exc), parent=dialog)

        ttk.Button(pane, text="Speichern", command=save).pack(pady=12)
        surface.bind_wheel()

    def person_details(self) -> None:
        person = self.selected_person()
        if not person:
            return
        data = next(p for p in self.snapshot["persons"] if p["id"] == person)
        dialog = tk.Toplevel(self)
        dialog.title(f"Person #{person}")
        dialog.geometry("850x750")
        surface = ScrollFrame(dialog)
        surface.pack(fill="both", expand=True)
        pane = surface.content
        fields = {}
        for key, label in (
            ("name", "Name"),
            ("birth_year", "Geburtsjahr"),
            ("sex", "Geschlecht"),
            ("note", "Notiz"),
        ):
            ttk.Label(pane, text=label).pack(anchor="w")
            fields[key] = tk.StringVar(value=data["attrs"].get(key, ""))
            ttk.Entry(pane, textvariable=fields[key]).pack(fill="x")

        def save_attrs():
            attrs = {key: value.get() for key, value in fields.items()}
            self.controller.run(lambda s: (s.set_attrs(person, attrs), s.rematch()))

        ttk.Button(pane, text="Attribute speichern", command=save_attrs).pack(pady=6)
        ttk.Label(
            pane,
            text=f"Datensatz: {len(self.snapshot['dataset'])} Gesichter · "
            f"{sum(r['split'] == 'ref' for r in self.snapshot['dataset'])} ref · "
            f"{sum(r['split'] == 'val' for r in self.snapshot['dataset'])} val",
        ).pack(anchor="w")
        years = sorted(
            {
                r["captured_at"][:4]
                for r in self.snapshot["dataset"]
                if r.get("captured_at")
            }
        )
        ttk.Label(
            pane, text="Abgedeckte Jahre: " + (", ".join(years) or "Datum unbekannt")
        ).pack(anchor="w")
        people = {p["id"]: p for p in self.snapshot["persons"]}
        relationships = []
        terms = {
            "eltern": ("Vater", "Mutter", "Elternteil"),
            "kinder": ("Sohn", "Tochter", "Kind"),
            "geschwister": ("Bruder", "Schwester", "Geschwister"),
            "grosseltern": ("Großvater", "Großmutter", "Großelternteil"),
            "onkel_tanten": ("Onkel", "Tante", "Onkel/Tante"),
        }
        for kind, ids in self.snapshot["relations"].items():
            for related in ids:
                attrs = people.get(related, {}).get("attrs", {})
                sex = attrs.get("sex", "").casefold()
                label = terms.get(kind, (kind, kind, kind))[
                    0
                    if sex in ("m", "male", "männlich")
                    else 1
                    if sex in ("f", "w", "female", "weiblich")
                    else 2
                ]
                relationships.append(
                    f"{label}: {attrs.get('name', 'Unbenannt')} · #{related}"
                )
        ttk.Label(
            pane,
            text="Abgeleitete Beziehungen\n"
            + ("\n".join(relationships) or "Noch keine Beziehungen"),
        ).pack(anchor="w")
        relation = tk.StringVar(value="elternteil_von")
        other = tk.StringVar()
        ttk.Combobox(
            pane,
            textvariable=relation,
            state="readonly",
            values=("elternteil_von", "kind_von", "partner_von", "geschwister_von"),
        ).pack(fill="x")
        ttk.Combobox(pane, textvariable=other, values=list(self.people)).pack(fill="x")

        def relate(remove=False):
            target, kind = self.people.get(other.get()), relation.get()
            if target:

                def operation(service):
                    with warnings.catch_warnings(record=True) as messages:
                        warnings.simplefilter("always", UserWarning)
                        with service.db:
                            (service.graph.remove if remove else service.graph.add)(
                                person, kind, target
                            )
                    return {"warnings": [str(message.message) for message in messages]}

                self.controller.run(operation)

        ttk.Button(pane, text="Beziehung hinzufügen", command=relate).pack()
        ttk.Button(
            pane, text="Beziehung entfernen", command=lambda: relate(True)
        ).pack()

        def merge():
            target = self.people.get(other.get())
            if target and messagebox.askyesno(
                "Personen zusammenführen",
                f"Person #{person} in #{target} zusammenführen?",
                parent=dialog,
            ):
                self.controller.run(lambda s: s.merge(person, target))
                dialog.destroy()

        ttk.Button(
            pane, text="Mit ausgewählter Person zusammenführen …", command=merge
        ).pack(pady=8)
        ttk.Label(pane, text="Fotos (alle Dateien der Foto-Paare)").pack(anchor="w")
        photos = tk.Text(pane, height=12, width=90)
        photos.pack(fill="both", expand=True)
        photos.insert(
            "end", "\n".join(row["relpath"] for row in self.snapshot["photos"])
        )
        photos.configure(state="disabled")
        surface.bind_wheel()

    def show_result(self, result) -> None:
        if isinstance(result, dict) and result.get("warnings"):
            self.status.set(" · ".join(result["warnings"]))
            return
        if isinstance(result, dict) and "person_id" in result:
            self.person.set(
                next(
                    (
                        name
                        for name, pid in self.people.items()
                        if pid == result["person_id"]
                    ),
                    "",
                )
            )
        elif isinstance(result, dict) and "available" in result:
            self.show_calibration(result)
        elif isinstance(result, dict) and "errors" in result:
            self.status.set(
                f"{'Abgebrochen' if result['cancelled'] else 'Analyse abgeschlossen'} · {result['done']} Fotos · "
                f"{result['skipped']} bereits fertig · {len(result['errors'])} Fehler"
                + ("\n" + "\n".join(result["errors"][:5]) if result["errors"] else "")
            )
        elif isinstance(result, dict) and "path" in result:
            self.status.set(f"{result['faces']} Gesichter exportiert: {result['path']}")

    def show_calibration(self, result) -> None:
        dialog = tk.Toplevel(self)
        dialog.title("Schwellenkalibrierung")
        if not result["available"]:
            ttk.Label(dialog, text=result["reason"], padding=20).pack()
            return
        ttk.Label(
            dialog,
            text=f"{result['samples']} Validierungsgesichter · Vorschlag auto: {result['proposed_auto']}\n{result['note']}",
            padding=12,
        ).pack()
        canvas = tk.Canvas(dialog, width=700, height=230, background="white")
        canvas.pack()
        histograms = [
            np.histogram(result[key], bins=20, range=(-1, 1))[0]
            for key in ("genuine", "impostor")
        ]
        largest = max(1, max(int(h.max()) for h in histograms))
        for group, (histogram, color) in enumerate(
            zip(histograms, ("#28744c", "#c66438"))
        ):
            for i, count in enumerate(histogram):
                x = 35 + i * 32 + group * 12
                canvas.create_rectangle(
                    x,
                    190 - int(count) / largest * 160,
                    x + 11,
                    190,
                    fill=color,
                    outline="",
                )
        canvas.create_text(
            350,
            215,
            text="Kosinus −1 bis +1 · grün: gleiche Person · orange: andere Person",
        )
        report = tk.Text(dialog, width=90, height=12)
        report.pack(fill="both", expand=True)
        report.insert(
            "end",
            json.dumps(
                {
                    "per_person": result["per_person"],
                    "precision_recall": [
                        r for r in result["curve"] if r["threshold"] >= 0.3
                    ][::5],
                },
                indent=2,
            ),
        )
        report.configure(state="disabled")

    def delete_all(self) -> None:
        if messagebox.askyesno(
            "Gesichtsdaten löschen",
            "Alle Gesichter, Embeddings, Personen, Beziehungen und Gesichtsexporte dieser Sammlung löschen?",
            parent=self,
        ):
            self.controller.run(lambda s: s.delete_all())
