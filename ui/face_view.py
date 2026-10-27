"""Advanced face tools alongside the group-first naming workspace."""

from __future__ import annotations

import json
import tkinter as tk
import warnings
from pathlib import Path
from tkinter import messagebox, ttk

import numpy as np

from ui.face_workspace import FaceWorkspace
from ui.scroll_frame import ScrollFrame


class FaceView(FaceWorkspace, ttk.Frame):
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
        if isinstance(result, dict) and "named_count" in result:
            self.status.set(
                f"{result['named_count']} Fotos als „{result['name']}“ zugeordnet. Weiter mit der nächsten Gruppe."
            )
            return
        if isinstance(result, dict) and "undone" in result:
            self.status.set("Letzte Zuordnung rückgängig gemacht.")
            return
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
