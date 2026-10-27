"""Group-first face naming workspace; advanced tools live in the options menu."""

from __future__ import annotations

import tkinter as tk
from tkinter import simpledialog, ttk

from PIL import ImageTk

from core.face_dataset import calibrate, export_dataset
from ui.face_group_row import FaceGroupRow
from ui.scroll_frame import ScrollFrame
from ui.theme import ACCENT


class FaceWorkspace:
    """Shared layout and interaction code for the face view's naming surface."""

    def __init__(self, parent, controller) -> None:
        super().__init__(parent, padding=(24, 16))
        self.controller = controller
        controller.view = self
        self.page = 0
        self.snapshot = {}
        self.images, self.cards, self.group_rows = [], [], []
        self.selected, self.people, self.preview_offsets = {}, {}, {}
        self._drafts = {}
        self._active_key = None
        self._focus_after_assign = None
        self._focus_after_undo = None
        self._pending_naming = False
        self.status = tk.StringVar(value="Ähnliche Gesichter gemeinsam benennen.")
        self.mode = tk.StringVar(value="Unbekannte")
        self.person = tk.StringVar()
        self.progress_text = tk.StringVar(value="Gesichter werden geladen …")
        self.page_text = tk.StringVar()
        style = ttk.Style(self)
        style.configure("FaceTab.TButton", padding=(20, 8), borderwidth=0)
        style.configure(
            "FaceTabActive.TButton",
            padding=(20, 8),
            foreground=ACCENT,
            background="#eaf2ff",
            font=("Segoe UI", 11, "bold"),
            borderwidth=0,
        )
        header = ttk.Frame(self)
        header.pack(fill="x", pady=(0, 10))
        ttk.Label(header, text="Gesichter & Personen", style="Heading.TLabel").pack(
            side="left"
        )
        self.naming_tab = ttk.Button(
            header, text="Benennen", command=lambda: self.set_mode("Unbekannte")
        )
        self.naming_tab.pack(side="left", padx=(32, 2))
        self.review_tab = ttk.Button(
            header, text="Prüfen", command=lambda: self.set_mode("Vorschläge")
        )
        self.review_tab.pack(side="left", padx=2)
        options = ttk.Menubutton(header, text="Weitere Optionen ▾")
        options.pack(side="right")
        menu = tk.Menu(options, tearoff=False)
        options.configure(menu=menu)
        menu.add_command(label="Aktualisieren", command=self.reload)
        menu.add_command(label="Ordner analysieren …", command=self.scan_folder)
        menu.add_command(label="Analyse abbrechen", command=controller.cancel.set)
        menu.add_separator()
        for label, mode in (
            ("Personen verwalten", "Person"),
            ("Referenzbilder", "Referenz"),
            ("Validierungsbilder", "Validierung"),
            ("Ausreißer prüfen", "Bitte prüfen"),
            ("Alle Gesichter", "Alle Gesichter"),
        ):
            menu.add_command(
                label=label, command=lambda value=mode: self.set_mode(value)
            )
        menu.add_separator()
        menu.add_command(
            label="Modelle / Einstellungen …", command=self.configure_models
        )
        menu.add_command(
            label="Datensatz neu aufbauen",
            command=lambda: controller.run(
                lambda s: (s.rebuild_training(), s.rematch())[0]
            ),
        )
        menu.add_command(
            label="Datensatz exportieren",
            command=lambda: controller.run(export_dataset),
        )
        menu.add_command(
            label="Kalibrierung …", command=lambda: controller.run(calibrate)
        )
        menu.add_separator()
        menu.add_command(label="Alle Gesichtsdaten löschen …", command=self.delete_all)
        ttk.Button(
            header, text="Fotos analysieren", command=lambda: controller.run("scan")
        ).pack(side="right", padx=8)
        title = ttk.Frame(self)
        title.pack(fill="x")
        self.heading = ttk.Label(
            title, text="Wer ist auf diesen Bildern?", style="Title.TLabel"
        )
        self.heading.pack(side="left")
        ttk.Label(title, textvariable=self.progress_text, style="Muted.TLabel").pack(
            side="right"
        )
        self.subtitle = ttk.Label(
            self,
            text="Ähnliche Gesichter sind bereits zusammengefasst. Namen tippen und Enter drücken.",
            style="Muted.TLabel",
        )
        self.subtitle.pack(anchor="w", pady=(4, 12))
        self._progress_fraction = 0
        self.progress = tk.Canvas(
            self, height=4, highlightthickness=0, background="#e8edf3"
        )
        self.progress.pack(fill="x", pady=(0, 10))
        self.progress.bind("<Configure>", self._draw_progress)
        self.person_bar = ttk.Frame(self)
        self.person_combo = ttk.Combobox(
            self.person_bar, textvariable=self.person, width=32, state="readonly"
        )
        self.person_combo.pack(side="left")
        self.person_combo.bind(
            "<<ComboboxSelected>>", lambda _: self.reload(reset=True)
        )
        ttk.Button(
            self.person_bar, text="Person / Beziehungen …", command=self.person_details
        ).pack(side="left", padx=6)
        ttk.Button(self.person_bar, text="Neue Person …", command=self.new_person).pack(
            side="left"
        )
        footer = ttk.Frame(self)
        footer.pack(side="bottom", fill="x", pady=(12, 0))
        self.undo_button = ttk.Button(
            footer,
            text="↶ Rückgängig",
            command=controller.undo_naming,
            state="disabled",
        )
        self.undo_button.pack(side="left")
        ttk.Label(
            footer,
            text="Tab  Nächste Gruppe     Enter  Zuordnen     Strg+Z  Rückgängig",
            style="Small.TLabel",
        ).pack(side="left", padx=16)
        self.next_button = ttk.Button(
            footer, text="Weiter →", command=lambda: self.turn_page(1)
        )
        self.next_button.pack(side="right")
        ttk.Label(footer, textvariable=self.page_text, style="Small.TLabel").pack(
            side="right", padx=8
        )
        self.previous_button = ttk.Button(
            footer, text="← Zurück", command=lambda: self.turn_page(-1)
        )
        self.previous_button.pack(side="right")
        ttk.Label(
            self, textvariable=self.status, style="Small.TLabel", wraplength=1250
        ).pack(side="bottom", fill="x", pady=(5, 0))
        self.surface = ScrollFrame(self)
        self.surface.pack(fill="both", expand=True)
        self.actions = ttk.Frame(self)
        for label, action in (
            ("Bestätigen", "confirm"),
            ("Ablehnen", "reject"),
            ("Ignorieren", "ignore"),
            ("Fixieren", "pin"),
            ("Fixierung lösen", "unpin"),
            ("Aus Datensatz entfernen", "exclude_training"),
        ):
            ttk.Button(
                self.actions,
                text=label,
                command=lambda value=action: self.face_action(value),
            ).pack(side="left", padx=3)
        self.bind("<Control-z>", lambda _: controller.undo_naming())

    def selected_person(self) -> int | None:
        return self.people.get(self.person.get())

    def _draw_progress(self, event=None) -> None:
        self.progress.delete("all")
        self.progress.create_rectangle(
            0,
            0,
            self.progress.winfo_width() * self._progress_fraction,
            4,
            fill=ACCENT,
            outline="",
        )

    def destroy(self) -> None:
        """Detach the worker and release Tcl variables on the UI thread."""
        if self.controller.view is self:
            self.controller.view = None
        super().destroy()
        self.images.clear()
        self.selected.clear()
        for attr in ("status", "mode", "person", "progress_text", "page_text"):
            setattr(self, attr, None)

    def set_mode(self, mode: str) -> None:
        self.mode.set(mode)
        self.page = 0
        self._save_drafts()
        self._active_key = None
        self.reload()

    def _save_drafts(self) -> None:
        for row in self.group_rows:
            if row.winfo_exists():
                self._drafts[row.key] = row.state()

    def clear(self) -> None:
        for child in self.surface.content.winfo_children():
            child.destroy()
        self.images, self.cards, self.group_rows = [], [], []
        self.selected, self.people, self.preview_offsets = {}, {}, {}
        self._drafts.clear()
        self._active_key = self._focus_after_assign = self._focus_after_undo = None
        self.snapshot = {}
        self.person.set("")
        self.person_combo.configure(values=())
        self.progress_text.set("Gesichter werden geladen …")
        self.page = 0
        self.undo_button.configure(state="disabled")

    def reload(self, reset=False) -> None:
        if reset:
            self.page = 0
        self._save_drafts()
        self.controller.run()

    def render(self, snapshot: dict) -> None:
        self._save_drafts()
        self.snapshot, self.page = snapshot, snapshot["page"]
        if not snapshot["status"]["faces"]:
            self._drafts.clear()
        self.people = {
            f"{p['attrs'].get('name', 'Unbenannt')} · #{p['id']}": p["id"]
            for p in snapshot["persons"]
        }
        self.person_combo.configure(values=list(self.people))
        if not self.people:
            self.person.set("")
        for child in self.surface.content.winfo_children():
            child.destroy()
        self.images, self.cards, self.group_rows = [], [], []
        self.selected = {}
        mode = self.mode.get()
        grouped = mode in ("Unbekannte", "Vorschläge")
        self.naming_tab.configure(
            style="FaceTabActive.TButton" if mode == "Unbekannte" else "FaceTab.TButton"
        )
        self.review_tab.configure(
            style="FaceTabActive.TButton" if mode == "Vorschläge" else "FaceTab.TButton"
        )
        if grouped:
            self.person_bar.pack_forget()
            self.actions.pack_forget()
            self.heading.configure(
                text="Wer ist auf diesen Bildern?"
                if mode == "Unbekannte"
                else "Stimmen diese Namen?"
            )
            self.subtitle.configure(
                text="Ähnliche Gesichter sind bereits zusammengefasst. Namen tippen und Enter drücken."
                if mode == "Unbekannte"
                else "Vorschläge gemeinsam bestätigen. Falsche Gesichter abwählen oder den Namen korrigieren."
            )
            self._render_groups(snapshot)
        else:
            self.person_bar.pack(before=self.surface, fill="x", pady=(0, 10))
            self.actions.pack(after=self.surface, fill="x", pady=8)
            self.heading.configure(text=mode)
            self.subtitle.configure(
                text="Person auswählen und Bilder oder Beziehungen verwalten."
            )
            self._render_faces(snapshot)
        status = snapshot["status"]
        total, named = status["faces"], snapshot.get("named_faces", 0)
        self.progress_text.set(f"{named} von {total} Gesichtern benannt")
        self._progress_fraction = named / max(1, total)
        self._draw_progress()
        pages = snapshot.get("pages", max(1, (snapshot["total"] + 59) // 60))
        self.page_text.set(f"Seite {self.page + 1} von {pages}")
        self.previous_button.configure(state="normal" if self.page else "disabled")
        self.next_button.configure(
            state="normal" if self.page + 1 < pages else "disabled"
        )
        self.undo_button.configure(
            state="normal" if self.controller.can_undo else "disabled"
        )
        self.status.set(
            "Ausreißer abwählen → Namen eingeben → Enter. Die Auswahl umfasst auch weitere Bilder der Gruppe."
            if grouped
            else f"{snapshot['total']} Bilder"
        )
        self.surface.bind_wheel()
        self.surface.canvas.update_idletasks()
        if self._focus_after_assign is not None:
            index = min(self._focus_after_assign, max(0, len(self.group_rows) - 1))
            self._focus_after_assign = None
            if self.group_rows:
                self.focus_group(self.group_rows[index])
        elif self._focus_after_undo is not None:
            face_ids = self._focus_after_undo
            self._focus_after_undo = None
            row = next(
                (r for r in self.group_rows if face_ids.intersection(r.key)), None
            )
            if row:
                self.focus_group(row)
        elif self.group_rows:
            row = next(
                (r for r in self.group_rows if r.key == self._active_key),
                self.group_rows[0],
            )
            self.focus_group(row)
        self._pending_naming = False

    def _render_groups(self, snapshot: dict) -> None:
        by_id = {face["id"]: face for face in snapshot["rows"]}
        for number, group in enumerate(
            snapshot["groups"], snapshot.get("group_start", 0) + 1
        ):
            faces = [by_id[face] for face in group if face in by_id]
            if not faces:
                continue
            row = FaceGroupRow(
                self.surface.content,
                faces=faces,
                number=number,
                people=snapshot["persons"],
                assign=self.assign_group,
                activate=self.activate_group,
                next_group=self.next_group,
                preview=self.preview_group,
                state=self._drafts.get(tuple(group)),
                review=self.mode.get() == "Vorschläge",
                offset=self.preview_offsets.get(group[0], 0),
            )
            row.pack(fill="x", pady=(0, 8))
            row.entry.bind("<Control-z>", lambda _: self._undo_key())
            self.group_rows.append(row)
        if not self.group_rows:
            message = (
                "Alle Gruppen sind benannt. Unter „Prüfen“ kannst du Vorschläge bestätigen."
                if snapshot["status"]["faces"]
                else "Noch keine Gesichter vorhanden. Starte mit „Fotos analysieren“."
            )
            ttk.Label(
                self.surface.content,
                text=message,
                style="Heading.TLabel",
                padding=32,
                wraplength=900,
            ).pack(anchor="w")

    def _render_faces(self, snapshot: dict) -> None:
        for position, face in enumerate(snapshot["rows"]):
            card = ttk.Frame(self.surface.content, padding=8)
            card.grid(
                row=position // 6, column=position % 6, sticky="nw", padx=4, pady=4
            )
            self.cards.append(card)
            variable = tk.BooleanVar()
            self.selected[face["id"]] = variable
            if face.get("image"):
                photo = ImageTk.PhotoImage(face["image"], master=self)
                self.images.append(photo)
                label = ttk.Label(card, image=photo)
            else:
                label = ttk.Label(card, text="Vorschau fehlt", width=14)
            label.pack()
            label.bind(
                "<Button-1>", lambda _, value=variable: value.set(not value.get())
            )
            ttk.Checkbutton(
                card, text=face["relpath"].rsplit("/", 1)[-1], variable=variable
            ).pack()

    def activate_group(self, row: FaceGroupRow) -> None:
        self._active_key = row.key
        for item in self.group_rows:
            item.set_active(item is row)

    def focus_group(self, row: FaceGroupRow) -> None:
        self.activate_group(row)
        row.focus_name()
        self.surface.canvas.update_idletasks()
        height = max(1, self.surface.content.winfo_height())
        top = self.surface.canvas.canvasy(0)
        bottom = top + self.surface.canvas.winfo_height()
        if row.winfo_y() < top or row.winfo_y() + row.winfo_height() > bottom:
            self.surface.canvas.yview_moveto(max(0, row.winfo_y() - 8) / height)

    def next_group(self, row: FaceGroupRow, direction: int = 1) -> None:
        index = self.group_rows.index(row) + direction
        if 0 <= index < len(self.group_rows):
            self.focus_group(self.group_rows[index])
        elif direction > 0 and self.page + 1 < self.snapshot.get("pages", 1):
            self._focus_after_assign = 0
            self.turn_page(1)
        elif direction < 0 and self.page:
            self._focus_after_assign = 100
            self.turn_page(-1)

    def preview_group(self, row: FaceGroupRow, offset: int) -> None:
        self._save_drafts()
        self.preview_offsets[row.key[0]] = offset
        self._active_key = row.key
        self.reload()

    def assign_group(self, row: FaceGroupRow) -> None:
        if self._pending_naming or self.controller.busy:
            self.status.set("Zuordnung wird gespeichert …")
            return
        name, ids = row.name.get().strip(), row.selected_ids()
        if not name or not ids:
            self.status.set(
                "Bitte einen Namen eingeben und mindestens ein Gesicht ausgewählt lassen."
            )
            row.focus_name()
            return
        self._save_drafts()
        index = self.group_rows.index(row)
        # Abgewählte Ausreißer bleiben in dieser Zeile. Der Fokus springt trotzdem
        # zur nächsten Gruppe; ohne Ausreißer rückt sie an die aktuelle Position.
        self._focus_after_assign = index + int(bool(row.excluded))
        if self.controller.assign_group(ids, name, row.person_id, list(row.excluded)):
            self._pending_naming = True
            self._drafts.pop(row.key, None)
            row.button.configure(state="disabled")
        else:
            self._focus_after_assign = None

    def _undo_key(self) -> str:
        self.controller.undo_naming()
        return "break"

    def ids(self) -> list[int]:
        return [face for face, variable in self.selected.items() if variable.get()]

    def assign(self) -> None:
        if self.group_rows:
            row = next(
                (r for r in self.group_rows if r.key == self._active_key),
                self.group_rows[0],
            )
            self.assign_group(row)
        else:
            ids, person = self.ids(), self.selected_person()
            if ids and person:
                self.controller.run(lambda s: s.assign(ids, person))

    def face_action(self, action: str) -> None:
        ids = self.ids()
        if not ids:
            self.status.set("Bitte zuerst Bilder auswählen.")
            return

        def operation(service):
            for face in ids:
                if action == "unpin":
                    service.pin(face, False)
                else:
                    getattr(service, action)(face)

        self.controller.run(operation)

    def turn_page(self, delta: int) -> None:
        self._save_drafts()
        self.page = max(0, min(self.snapshot.get("pages", 1) - 1, self.page + delta))
        self.surface.canvas.yview_moveto(0)
        self.reload()

    def new_person(self) -> None:
        name = simpledialog.askstring("Person anlegen", "Name:", parent=self)
        if name is not None and name.strip():
            self.controller.run(
                lambda s: {"person_id": s.create_person({"name": name.strip()})}
            )
