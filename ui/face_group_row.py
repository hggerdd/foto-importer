"""One visual group, one inline name, one assignment action."""

from __future__ import annotations

import tkinter as tk
from collections.abc import Callable
from tkinter import ttk

from PIL import ImageTk

from config.settings import FACE_GROUP_PREVIEWS
from ui.theme import ACCENT, MUTED, TEXT


class FaceGroupRow(tk.Frame):
    """Keep group selection and name drafts local; emit explicit UI intentions."""

    def __init__(
        self,
        parent: tk.Misc,
        *,
        faces: list[dict],
        number: int,
        people: list[dict],
        assign: Callable,
        activate: Callable,
        next_group: Callable,
        preview: Callable,
        review=False,
        state: dict | None = None,
        offset=0,
    ) -> None:
        super().__init__(
            parent,
            background="white",
            highlightthickness=1,
            highlightbackground="#dce1e8",
            highlightcolor=ACCENT,
            padx=16,
            pady=8,
        )
        self.faces, self.people = faces, people
        self.key = tuple(face["id"] for face in faces)
        self.on_assign, self.on_activate = assign, activate
        self.on_next, self.on_preview = next_group, preview
        self.review = review
        state = state or {}
        self.excluded = set(state.get("excluded", ()))
        self.person_id = state.get("person_id")
        self.name = tk.StringVar(value=state.get("name", ""))
        if not state and review and faces:
            self.person_id = faces[0]["person_id"]
            self.name.set(
                next(
                    (
                        p["attrs"].get("name", "")
                        for p in people
                        if p["id"] == self.person_id
                    ),
                    "",
                )
            )
        self.matches = []
        self.images, self.tiles = [], {}
        self._choosing = False
        self._choice_after = None
        self._destroying = False
        self.offset = offset
        self.columnconfigure(0, weight=1)
        self.columnconfigure(1, minsize=265)
        years = sorted(
            {str(face["captured_at"])[:4] for face in faces if face.get("captured_at")}
        )
        span = (
            f" · {years[0]}–{years[-1]}"
            if len(years) > 1
            else f" · {years[0]}"
            if years
            else ""
        )
        header = ttk.Frame(self)
        header.grid(row=0, column=0, sticky="ew", padx=(0, 20), pady=(0, 8))
        ttk.Label(header, text=f"Gruppe {number}", style="Heading.TLabel").pack(
            side="left"
        )
        ttk.Label(
            header, text=f"  {len(faces)} Fotos{span}", style="Muted.TLabel"
        ).pack(side="left")
        self.strip = ttk.Frame(self)
        self.strip.grid(row=1, column=0, sticky="nsew", padx=(0, 20))
        self.strip.bind("<Configure>", self._reflow)
        visible = faces[offset : offset + FACE_GROUP_PREVIEWS]
        for position, face in enumerate(visible):
            tile = tk.Frame(self.strip, background="white", highlightthickness=2)
            tile.grid(row=0, column=position, padx=(0, 6), pady=(0, 4))
            if face.get("image"):
                photo = ImageTk.PhotoImage(face["image"], master=self)
                self.images.append(photo)
                label = tk.Label(
                    tile,
                    image=photo,
                    borderwidth=0,
                    cursor="hand2",
                    background="#eef1f5",
                )
            else:
                label = tk.Label(
                    tile,
                    text="Vorschau\nfehlt",
                    width=12,
                    height=6,
                    background="#eef1f5",
                    cursor="hand2",
                )
            label.pack()
            check = tk.Label(
                tile, text="✓", font=("Segoe UI", 10, "bold"), cursor="hand2"
            )
            check.place(relx=1, x=-3, y=3, anchor="ne", width=22, height=22)
            for widget in (label, check):
                widget.bind("<Button-1>", lambda _, fid=face["id"]: self.toggle(fid))
            self.tiles[face["id"]] = (tile, check)
        hints = ttk.Frame(self)
        hints.grid(row=2, column=0, sticky="ew", padx=(0, 20), pady=(4, 0))
        self.selection_label = ttk.Label(hints, style="Muted.TLabel")
        self.selection_label.pack(side="left")
        if len(faces) > FACE_GROUP_PREVIEWS:
            ttk.Button(
                hints,
                text="←",
                width=3,
                style="Link.TButton",
                state="normal" if offset else "disabled",
                command=lambda: self.on_preview(
                    self, max(0, offset - FACE_GROUP_PREVIEWS)
                ),
            ).pack(side="right")
            ttk.Button(
                hints,
                text="Weitere Bilder →",
                style="Link.TButton",
                state="normal"
                if offset + FACE_GROUP_PREVIEWS < len(faces)
                else "disabled",
                command=lambda: self.on_preview(self, offset + FACE_GROUP_PREVIEWS),
            ).pack(side="right")
            ttk.Label(
                hints,
                text=f"{offset + 1}–{min(offset + FACE_GROUP_PREVIEWS, len(faces))} von {len(faces)}",
                style="Muted.TLabel",
            ).pack(side="right", padx=6)
        controls = ttk.Frame(self, width=265)
        controls.grid(row=0, column=1, rowspan=3, sticky="new")
        ttk.Label(controls, text="Name", style="Small.TLabel").pack(
            anchor="w", pady=(0, 4)
        )
        self.entry = ttk.Entry(controls, textvariable=self.name, width=28)
        self.entry.pack(fill="x")
        self.entry.bind("<FocusIn>", lambda _: self._focus())
        self.entry.bind("<Return>", self._enter)
        self.entry.bind("<Tab>", lambda _: self._tab(1))
        self.entry.bind("<Shift-Tab>", lambda _: self._tab(-1))
        self.entry.bind("<Down>", lambda _: self._move_match(1))
        self.entry.bind("<Up>", lambda _: self._move_match(-1))
        self.entry.bind("<Escape>", lambda _: self.hide_choices())
        self.entry.bind("<FocusOut>", self._focus_out)
        self.choices = tk.Listbox(
            controls,
            height=3,
            activestyle="none",
            exportselection=False,
            relief="flat",
            highlightthickness=1,
            highlightbackground="#dce1e8",
            font=("Segoe UI", 10),
            selectbackground="#e7f0ff",
            selectforeground=TEXT,
            takefocus=False,
        )
        self.choices.bind("<ButtonPress-1>", self._click_choice)
        self.button = ttk.Button(controls, style="Primary.TButton", command=self.submit)
        self.button.pack(fill="x", pady=(8, 0))
        self._name_trace = self.name.trace_add("write", self._name_changed)
        self._selection_changed()
        self._choosing = True
        self._name_changed()
        self._choosing = False

    def state(self) -> dict:
        return {
            "name": self.name.get(),
            "person_id": self.person_id,
            "excluded": set(self.excluded),
        }

    def selected_ids(self) -> list[int]:
        return [face["id"] for face in self.faces if face["id"] not in self.excluded]

    def toggle(self, face_id: int) -> None:
        if face_id in self.excluded:
            self.excluded.remove(face_id)
        else:
            self.excluded.add(face_id)
        self.on_activate(self)
        self._selection_changed()
        self.entry.focus_set()

    def _selection_changed(self) -> None:
        count = len(self.selected_ids())
        for face_id, (tile, check) in self.tiles.items():
            selected = face_id not in self.excluded
            tile.configure(highlightbackground=ACCENT if selected else "#d9dde3")
            check.configure(
                text="✓" if selected else "−",
                background=ACCENT if selected else "#e4e7eb",
                foreground="white" if selected else MUTED,
            )
        suffix = f" · {len(self.excluded)} abgewählt" if self.excluded else ""
        self.selection_label.configure(
            text=f"{count} ausgewählt{suffix} · Ausreißer anklicken zum Abwählen"
        )
        self.button.configure(
            text=f"{count} Fotos {'bestätigen' if self.review else 'zuordnen'}  ↵",
            state="normal" if count and self.name.get().strip() else "disabled",
        )

    def set_active(self, active: bool) -> None:
        self.configure(
            highlightbackground=ACCENT if active else "#dce1e8",
            highlightthickness=2 if active else 1,
        )

    def focus_name(self) -> None:
        self.entry.focus_set()
        self.entry.icursor("end")

    def _focus(self) -> None:
        self.on_activate(self)
        self._show_choices()

    def _name_changed(self, *args) -> None:
        if not self._choosing:
            self.person_id = None
        self._selection_changed()
        self._show_choices()

    def _show_choices(self) -> None:
        query = self.name.get().strip().casefold()
        self.matches = [
            p
            for p in self.people
            if query and query in p["attrs"].get("name", "").casefold()
        ][:4]
        self.choices.delete(0, "end")
        for person in self.matches:
            attrs = person["attrs"]
            name = attrs.get("name", "Unbenannt")
            duplicate = (
                sum(
                    p["attrs"].get("name", "").casefold() == name.casefold()
                    for p in self.people
                )
                > 1
            )
            suffix = (
                f" · {attrs.get('birth_year') or '#' + str(person['id'])}"
                if duplicate
                else ""
            )
            self.choices.insert("end", name + suffix)
        if query and not any(
            p["attrs"].get("name", "").strip().casefold() == query for p in self.people
        ):
            self.choices.insert("end", f"+ „{self.name.get().strip()}“ neu anlegen")
        if self.entry.focus_get() == self.entry and self.choices.size():
            self.choices.configure(height=min(4, self.choices.size()))
            self.choices.pack(fill="x", before=self.button, pady=(2, 0))
        else:
            self.hide_choices()

    def hide_choices(self) -> None:
        self.choices.pack_forget()

    def _hide_unfocused_choices(self) -> None:
        self._choice_after = None
        if self.winfo_exists() and self.focus_get() not in (self.entry, self.choices):
            self.hide_choices()

    def _focus_out(self, event=None) -> None:
        if self._destroying:
            return
        if self._choice_after:
            self.after_cancel(self._choice_after)
        self._choice_after = self.after_idle(self._hide_unfocused_choices)

    def destroy(self) -> None:
        """Release Tcl traces and images on Tk's thread before worker GC can run."""
        self._destroying = True
        if self._choice_after:
            self.after_cancel(self._choice_after)
            self._choice_after = None
        self.name.trace_remove("write", self._name_trace)
        self.images.clear()
        super().destroy()
        self.name = None

    def _move_match(self, direction: int) -> str:
        self._show_choices()
        if self.choices.size():
            selected = self.choices.curselection()
            index = (selected[0] + direction) % self.choices.size() if selected else 0
            self.choices.selection_clear(0, "end")
            self.choices.selection_set(index)
        return "break"

    def _choose(self, index: int) -> None:
        self._choosing = True
        if index < len(self.matches):
            person = self.matches[index]
            self.name.set(person["attrs"].get("name", ""))
            self.person_id = person["id"]
        self._choosing = False
        self.hide_choices()

    def _click_choice(self, event) -> str:
        self._choose(self.choices.nearest(event.y))
        self.entry.focus_set()
        return "break"

    def _enter(self, event=None) -> str:
        selected = self.choices.curselection()
        if self.choices.winfo_manager() and selected:
            self._choose(selected[0])
        self.submit()
        return "break"

    def _tab(self, direction: int) -> str:
        self.hide_choices()
        self.on_next(self, direction)
        return "break"

    def submit(self) -> None:
        self.hide_choices()
        self.on_assign(self)

    def _reflow(self, event=None) -> None:
        columns = max(1, self.strip.winfo_width() // 116)
        for position, (tile, _) in enumerate(self.tiles.values()):
            tile.grid_configure(row=position // columns, column=position % columns)
