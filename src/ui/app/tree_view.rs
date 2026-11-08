//! Stammbaum, Verwandtschaftsprüfung, Rückfragen und Personen anlegen (ADR 0061).
//!
//! Alle Verwandtschaft wird aus den gespeicherten Basisfakten berechnet
//! (`core::kinship`, `core::tree_layout`); die Oberfläche zeichnet nur.
use super::*;
use crate::core::gaps::{self, Answer, Suggestion, SuggestionKind};
use crate::core::kinship::{Family, Relation};
use crate::core::relations::{self, Gender, Role};
use crate::core::tree_layout::{self, LinkKind, NodeRole};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

const UNIT_X: f32 = 168.0;
const UNIT_Y: f32 = 104.0;
const BOX_W: f32 = 152.0;
const BOX_H: f32 = 50.0;
const PAD: f32 = 24.0;

/// Zustand der Familienansichten (Stammbaum, Prüfwerkzeug, Formulare).
pub(super) struct FamilyUi {
    pub(super) focus: Option<i64>,
    pub(super) up: u8,
    pub(super) down: u8,
    check_a: Option<i64>,
    check_b: Option<i64>,
    pub(super) new_name: String,
    new_gender: String,
    new_birth: String,
    new_note: String,
    /// Zuletzt angelegte Person: bietet direkt "Beziehung hinzufügen" an.
    pub(super) just_added: Option<i64>,
    quick_other: Option<i64>,
    snoozed: BTreeSet<String>,
}

impl Default for FamilyUi {
    fn default() -> Self {
        Self {
            focus: None,
            up: 2,
            down: 2,
            check_a: None,
            check_b: None,
            new_name: String::new(),
            new_gender: String::new(),
            new_birth: String::new(),
            new_note: String::new(),
            just_added: None,
            quick_other: None,
            snoozed: BTreeSet::new(),
        }
    }
}

fn gender_color(gender: Gender) -> Color32 {
    match gender {
        Gender::Female => Color32::from_rgb(214, 120, 160),
        Gender::Male => Color32::from_rgb(90, 140, 210),
        Gender::Unknown => Color32::from_rgb(140, 140, 150),
    }
}

fn initials(name: &str) -> String {
    let mut letters: Vec<char> = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .collect();
    if letters.len() > 2 {
        letters = vec![letters[0], letters[letters.len() - 1]];
    }
    letters.iter().flat_map(|c| c.to_uppercase()).collect()
}

fn dashed(painter: &egui::Painter, points: [Pos2; 2], stroke: Stroke) {
    for shape in egui::Shape::dashed_line(&points, stroke, 5.0, 4.0) {
        painter.add(shape);
    }
}

impl PhotoApp {
    fn person_name(&self, id: i64) -> String {
        self.face_page
            .people
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| format!("#{id}"), |p| p.name.clone())
    }

    fn person_combo(
        ui: &mut egui::Ui,
        salt: &str,
        label: &str,
        people: &[faces::Person],
        selected: &mut Option<i64>,
    ) {
        let caption = selected
            .and_then(|id| people.iter().find(|p| p.id == id))
            .map_or_else(
                || format!("{label}: wählen …"),
                |p| format!("{label}: {} · #{}", p.name, p.id),
            );
        egui::ComboBox::from_id_salt(salt)
            .selected_text(caption)
            .width(240.0)
            .show_ui(ui, |ui| {
                for p in people {
                    ui.selectable_value(selected, Some(p.id), format!("{} · #{}", p.name, p.id));
                }
            });
    }

    /// Standard-Fokus: die Person mit den meisten Familienbeziehungen.
    fn default_focus(&self) -> Option<i64> {
        let family = &self.face_page.family;
        self.face_page
            .people
            .iter()
            .max_by_key(|p| {
                family.parents_of(p.id).len()
                    + family.children_of(p.id).len()
                    + family.partners_of(p.id).len()
            })
            .map(|p| p.id)
    }

    /// Reiter „Stammbaum“: Baum um eine Fokus-Person, Verwandtschaftsprüfung, Textliste.
    pub(super) fn tree_tab(&mut self, ui: &mut egui::Ui) {
        if self.face_page.people.is_empty() {
            ui.label(
                RichText::new(
                    "Noch keine Personen. Lege im Reiter „Personen & Beziehungen“ Personen an.",
                )
                .weak(),
            );
            return;
        }
        if self.family_ui.focus.is_none() {
            self.family_ui.focus = self.relation_from.or_else(|| self.default_focus());
        }
        let people = self.face_page.people.clone();
        let mut ui_state = std::mem::take(&mut self.family_ui);
        ui.horizontal_wrapped(|ui| {
            Self::person_combo(ui, "tree-focus", "Person", &people, &mut ui_state.focus);
            ui.add(
                egui::Slider::new(&mut ui_state.up, 0..=tree_layout::MAX_TREE_GENERATIONS)
                    .text("Generationen oben"),
            );
            ui.add(
                egui::Slider::new(&mut ui_state.down, 0..=tree_layout::MAX_TREE_GENERATIONS)
                    .text("unten"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Verwandtschaft prüfen").strong());
            Self::person_combo(ui, "check-a", "Person A", &people, &mut ui_state.check_a);
            Self::person_combo(ui, "check-b", "Person B", &people, &mut ui_state.check_b);
        });
        let family = &self.face_page.family;
        let name = |id: i64| {
            people
                .iter()
                .find(|p| p.id == id)
                .map_or_else(|| format!("#{id}"), |p| p.name.clone())
        };
        if let (Some(a), Some(b)) = (ui_state.check_a, ui_state.check_b)
            && a != b
        {
            ui.label(RichText::new(family.sentence(a, b, &name)).strong());
            if let Some(link) = family.relationship(a, b) {
                ui.label(RichText::new(format!("Weg: {}", family.path_text(&link, &name))).weak());
            }
        }
        let mut refocus = None;
        if let Some(focus) = ui_state.focus {
            let layout = tree_layout::layout(family, focus, ui_state.up, ui_state.down);
            if layout.truncated > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} weitere Personen werden nicht gezeichnet; verringere die Generationen.",
                        layout.truncated
                    ))
                    .weak(),
                );
            }
            if layout.nodes.len() <= 1 {
                ui.label(RichText::new("Zu dieser Person sind noch keine Eltern, Kinder oder Partner erfasst. Beziehungen trägst du im Reiter „Personen & Beziehungen“ ein.").weak());
            }
            let needed = (layout.height - 1.0) * UNIT_Y + BOX_H + 2.0 * PAD + 20.0;
            let height = needed.min((ui.available_height() - 50.0).max(180.0));
            egui::ScrollArea::both()
                .id_salt("family-tree")
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if let Some(person) = Self::draw_tree(ui, family, &people, &layout, focus) {
                        refocus = Some(person);
                    }
                });
            egui::CollapsingHeader::new("Als Liste anzeigen (Textalternative)")
                .id_salt("tree-list")
                .show(ui, |ui| {
                    if let Some(person) = Self::relatives_list(ui, family, &people, focus) {
                        refocus = Some(person);
                    }
                });
        }
        if let Some(person) = refocus {
            ui_state.focus = Some(person);
            self.relation_from = Some(person);
        }
        self.family_ui = ui_state;
    }

    fn draw_tree(
        ui: &mut egui::Ui,
        family: &Family,
        people: &[faces::Person],
        layout: &tree_layout::TreeLayout,
        focus: i64,
    ) -> Option<i64> {
        let size = Vec2::new(
            layout.width * UNIT_X + 2.0 * PAD,
            (layout.height - 1.0) * UNIT_Y + BOX_H + 2.0 * PAD,
        );
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals().clone();
        let line = Stroke::new(1.6_f32, visuals.widgets.noninteractive.fg_stroke.color);
        let partner_line = Stroke::new(2.2_f32, Color32::from_rgb(200, 110, 90));
        let at = |x: f32, y: f32| {
            Pos2::new(
                rect.left() + PAD + x * UNIT_X,
                rect.top() + PAD + BOX_H / 2.0 + y * UNIT_Y,
            )
        };
        let centre = |id: i64| layout.node(id).map(|n| at(n.x, n.y));
        for link in &layout.links {
            match link {
                LinkKind::Parents { child, parents } => {
                    let (Some(c), pts) = (
                        centre(*child),
                        parents
                            .iter()
                            .filter_map(|p| centre(*p))
                            .collect::<Vec<_>>(),
                    ) else {
                        continue;
                    };
                    if pts.is_empty() {
                        continue;
                    }
                    let bar_y = pts[0].y + BOX_H / 2.0 + 14.0;
                    let jx = pts.iter().map(|p| p.x).sum::<f32>() / pts.len() as f32;
                    let (lo, hi) = pts
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.x), h.max(p.x)));
                    for p in &pts {
                        painter.line_segment(
                            [Pos2::new(p.x, p.y + BOX_H / 2.0), Pos2::new(p.x, bar_y)],
                            line,
                        );
                    }
                    painter.line_segment([Pos2::new(lo, bar_y), Pos2::new(hi, bar_y)], line);
                    let top = c.y - BOX_H / 2.0;
                    let mid = (bar_y + top) / 2.0;
                    painter.line_segment([Pos2::new(jx, bar_y), Pos2::new(jx, mid)], line);
                    painter.line_segment([Pos2::new(jx, mid), Pos2::new(c.x, mid)], line);
                    painter.line_segment([Pos2::new(c.x, mid), Pos2::new(c.x, top)], line);
                }
                LinkKind::Partner { a, b } | LinkKind::FormerPartner { a, b } => {
                    let former = matches!(link, LinkKind::FormerPartner { .. });
                    let (Some(pa), Some(pb)) = (centre(*a), centre(*b)) else {
                        continue;
                    };
                    let (left, right) = if pa.x <= pb.x { (pa, pb) } else { (pb, pa) };
                    let stroke = if former {
                        Stroke::new(1.6_f32, Color32::from_rgb(150, 150, 160))
                    } else {
                        partner_line
                    };
                    let points = if right.x - left.x < UNIT_X * 1.3 {
                        [
                            Pos2::new(left.x + BOX_W / 2.0, left.y),
                            Pos2::new(right.x - BOX_W / 2.0, right.y),
                        ]
                    } else {
                        // Nicht benachbart: Klammer unterhalb der Kästchen.
                        let y = left.y + BOX_H / 2.0 + 5.0;
                        painter.line_segment(
                            [
                                Pos2::new(left.x, left.y + BOX_H / 2.0),
                                Pos2::new(left.x, y),
                            ],
                            stroke,
                        );
                        painter.line_segment(
                            [
                                Pos2::new(right.x, right.y + BOX_H / 2.0),
                                Pos2::new(right.x, y),
                            ],
                            stroke,
                        );
                        [Pos2::new(left.x, y), Pos2::new(right.x, y)]
                    };
                    if former {
                        dashed(&painter, points, stroke);
                    } else {
                        painter.line_segment(points, stroke);
                    }
                }
            }
        }
        let mut clicked = None;
        for node in &layout.nodes {
            let c = at(node.x, node.y);
            let boxed = Rect::from_center_size(c, Vec2::new(BOX_W, BOX_H));
            let person = people.iter().find(|p| p.id == node.person);
            let name = person.map_or_else(|| format!("#{}", node.person), |p| p.name.clone());
            let gender = family.gender(node.person);
            let response = ui.interact(
                boxed,
                ui.id().with(("tree-node", node.person)),
                Sense::click(),
            );
            let fill = match node.role {
                NodeRole::Focus => visuals.selection.bg_fill,
                NodeRole::FormerPartner => visuals.extreme_bg_color,
                _ => visuals.faint_bg_color.gamma_multiply(1.0),
            };
            painter.rect_filled(boxed, 6.0, fill);
            painter.rect_stroke(
                boxed,
                6.0,
                Stroke::new(
                    if response.hovered() || node.person == focus {
                        2.0_f32
                    } else {
                        1.0_f32
                    },
                    if node.person == focus {
                        visuals.selection.stroke.color
                    } else {
                        visuals.widgets.noninteractive.bg_stroke.color
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_size(boxed.min, Vec2::new(5.0, BOX_H)),
                3.0,
                gender_color(gender),
            );
            let badge = Pos2::new(boxed.left() + 24.0, c.y);
            painter.circle_filled(badge, 14.0, gender_color(gender).gamma_multiply(0.35));
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                initials(&name),
                FontId::proportional(12.0),
                visuals.text_color(),
            );
            let text_rect = Rect::from_min_max(
                Pos2::new(boxed.left() + 44.0, boxed.top()),
                boxed.right_bottom(),
            );
            let clipped = painter.with_clip_rect(text_rect);
            clipped.text(
                Pos2::new(text_rect.left(), c.y - 7.0),
                Align2::LEFT_CENTER,
                &name,
                FontId::proportional(12.5),
                visuals.strong_text_color(),
            );
            let sub = match person.map(|p| p.birth.as_str()).filter(|b| !b.is_empty()) {
                Some(year) => format!("* {year}"),
                None => match node.role {
                    NodeRole::FormerPartner => "früherer Partner".into(),
                    _ => String::new(),
                },
            };
            clipped.text(
                Pos2::new(text_rect.left(), c.y + 9.0),
                Align2::LEFT_CENTER,
                sub,
                FontId::proportional(11.0),
                visuals.weak_text_color(),
            );
            if node.more_up {
                painter.text(
                    Pos2::new(c.x, boxed.top() - 1.0),
                    Align2::CENTER_BOTTOM,
                    "▲",
                    FontId::proportional(10.0),
                    visuals.weak_text_color(),
                );
            }
            if node.more_down {
                painter.text(
                    Pos2::new(c.x, boxed.bottom() + 1.0),
                    Align2::CENTER_TOP,
                    "▼",
                    FontId::proportional(10.0),
                    visuals.weak_text_color(),
                );
            }
            let describe = if node.person == focus {
                format!("{name} (gewählte Person)")
            } else {
                let names = |id: i64| {
                    people
                        .iter()
                        .find(|p| p.id == id)
                        .map_or_else(|| format!("#{id}"), |p| p.name.clone())
                };
                family.sentence(node.person, focus, &names)
            };
            if response
                .on_hover_text(format!(
                    "{describe}\nKlick: diese Person in den Mittelpunkt"
                ))
                .clicked()
            {
                clicked = Some(node.person);
            }
        }
        clicked
    }

    /// Gruppierte Textliste aller berechneten Verwandten (barrierefreie Alternative
    /// zum gezeichneten Baum). Liefert die angeklickte Person.
    pub(super) fn relatives_list(
        ui: &mut egui::Ui,
        family: &Family,
        people: &[faces::Person],
        person: i64,
    ) -> Option<i64> {
        let mut picked = None;
        let groups = family.relatives_of(person);
        if groups.is_empty() {
            ui.label(RichText::new("Keine Verwandtschaft im Stammbaum erfasst.").weak());
        }
        for group in groups {
            ui.label(RichText::new(&group.title).strong());
            for member in &group.members {
                let name = people
                    .iter()
                    .find(|p| p.id == member.person)
                    .map_or_else(|| format!("#{}", member.person), |p| p.name.clone());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if ui.link(name).clicked() {
                        picked = Some(member.person);
                    }
                    ui.label(
                        RichText::new(member.relation.label(family.gender(member.person))).weak(),
                    );
                });
            }
            if group.more > 0 {
                ui.label(RichText::new(format!("    … {} weitere", group.more)).weak());
            }
        }
        picked
    }

    /// Eintragen von Beziehungen für `person`: „[Person] ist Kind von / Elternteil von /
    /// Partner von / Freund von [andere Person]“.
    pub(super) fn quick_relation_row(&mut self, ui: &mut egui::Ui, person: i64) {
        let busy = self.busy();
        let people = self.face_page.people.clone();
        let others: Vec<faces::Person> =
            people.iter().filter(|p| p.id != person).cloned().collect();
        let mut other = self.family_ui.quick_other.filter(|o| *o != person);
        let me = self.person_name(person);
        let mut action: Option<Role> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{me} ist"));
            for (role, label) in [
                (Role::Child, "Kind von"),
                (Role::Parent, "Elternteil von"),
                (Role::Partner, "Partner von"),
                (Role::Friend, "Freund von"),
            ] {
                if ui
                    .add_enabled(!busy && other.is_some(), egui::Button::new(label))
                    .clicked()
                {
                    action = Some(role);
                }
            }
            Self::person_combo(ui, "quick-other", "Person", &others, &mut other);
        });
        self.family_ui.quick_other = other;
        if let (Some(role), Some(object)) = (action, other) {
            let root = self.settings.collection.clone();
            self.undo.clear();
            self.launch("Beziehung speichern", true, "mutation", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                relations::add(&mut c, person, role, object)?;
                Ok(Outcome::Done("Beziehung gespeichert".into()))
            });
        }
    }

    /// Übersicht für Person A: gespeicherte Beziehungen (bearbeitbar) und die aus dem
    /// Stammbaum berechnete Verwandtschaft (nicht gespeichert, nicht bearbeitbar).
    pub(super) fn relation_overview(&mut self, ui: &mut egui::Ui) {
        let Some(person) = self.relation_from else {
            ui.label(RichText::new("Person A wählen, um ihre Beziehungen zu sehen.").weak());
            return;
        };
        let busy = self.busy();
        let people = self.face_page.people.clone();
        let Some(me) = people.iter().find(|p| p.id == person) else {
            return;
        };
        let info = |id: i64| {
            people.iter().find(|p| p.id == id).map_or_else(
                || (format!("#{id}"), Gender::Unknown),
                |p| (p.name.clone(), Gender::parse(&p.gender)),
            )
        };
        let direct = relations::direct(&self.face_page.relations, person);
        let family = self.face_page.family.clone();
        let mut action: Option<(i64, Role, i64)> = None;
        let mut select = None;
        ui.label(RichText::new(format!("Beziehungen von {}", me.name)).strong());
        self.quick_relation_row(ui, person);
        ui.label(RichText::new("Gespeichert").strong());
        if direct.is_empty() {
            ui.label(RichText::new("Keine Beziehungen eingetragen.").weak());
        }
        for role in Role::ALL {
            let items: Vec<_> = direct.iter().filter(|(r, _)| *r == role).collect();
            if items.is_empty() {
                continue;
            }
            ui.label(role.group_label());
            for (_, other) in items {
                ui.horizontal(|ui| {
                    let (name, gender) = info(*other);
                    ui.add_space(12.0);
                    if ui.link(name).clicked() {
                        select = Some(*other);
                    }
                    ui.label(RichText::new(format!("{} · #{other}", role.label(gender))).weak());
                    if ui
                        .add_enabled(!busy, egui::Button::new("Entfernen").small())
                        .clicked()
                    {
                        action = Some((*other, role, person));
                    }
                });
            }
        }
        for (a, kind, b) in family.redundant_recorded() {
            if a != person && b != person {
                continue;
            }
            let other = if a == person { b } else { a };
            let role = if kind == relations::Kind::Sibling {
                Role::Sibling
            } else {
                Role::Cousin
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Hinweis: Die direkt erfasste Verbindung „{}“ zu {} folgt jetzt aus dem Stammbaum und ist überflüssig.",
                        kind.title(),
                        info(other).0
                    ))
                    .weak(),
                );
                if ui
                    .add_enabled(!busy, egui::Button::new("Behelfsverbindung entfernen").small())
                    .clicked()
                {
                    action = Some((other, role, person));
                }
            });
        }
        let groups = family.relatives_of(person);
        let shown: Vec<_> = groups
            .iter()
            .filter(|g| {
                g.members.iter().any(|m| {
                    !matches!(
                        m.relation,
                        Relation::Partner
                            | Relation::FormerPartner
                            | Relation::RecordedSibling
                            | Relation::RecordedCousin
                    ) && m.relation
                        != (Relation::Blood {
                            a_gen: 0,
                            b_gen: 1,
                            half: false,
                        })
                        && m.relation
                            != (Relation::Blood {
                                a_gen: 1,
                                b_gen: 0,
                                half: false,
                            })
                })
            })
            .collect();
        if !shown.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Aus dem Stammbaum berechnet (nicht gespeichert)").strong());
            for group in shown {
                ui.label(&group.title);
                for member in &group.members {
                    ui.horizontal(|ui| {
                        let (name, gender) = info(member.person);
                        ui.add_space(12.0);
                        if ui.link(name).clicked() {
                            select = Some(member.person);
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} · #{}",
                                member.relation.label(gender),
                                member.person
                            ))
                            .italics()
                            .weak(),
                        );
                    });
                }
                if group.more > 0 {
                    ui.label(RichText::new(format!("    … {} weitere", group.more)).weak());
                }
            }
        }
        if select.is_some() {
            self.relation_from = select;
        }
        if let Some((subject, role, object)) = action {
            let root = self.settings.collection.clone();
            self.undo.clear();
            self.launch("Beziehung entfernen", true, "mutation", move |_| {
                let c = Collection::open(Path::new(&root), true)?;
                relations::remove(&c, subject, role, object)?;
                Ok(Outcome::Done("Beziehung entfernt".into()))
            });
        }
    }

    /// Liste der offenen Rückfragen zu wahrscheinlich fehlenden Eltern.
    pub(super) fn questions_list(&mut self, ui: &mut egui::Ui) {
        let open: Vec<Suggestion> = self
            .face_page
            .questions
            .iter()
            .filter(|s| !self.family_ui.snoozed.contains(&format!("{s:?}")))
            .cloned()
            .collect();
        if open.is_empty() {
            return;
        }
        let busy = self.busy();
        let family = self.face_page.family.clone();
        ui.label(RichText::new(format!("Rückfragen · {}", open.len())).strong());
        ui.label(
            RichText::new(
                "Das könnte im Stammbaum fehlen. Nichts wird ohne deine Antwort ergänzt.",
            )
            .weak(),
        );
        let mut answered: Option<(Suggestion, Answer)> = None;
        let mut snooze = None;
        for s in &open {
            let names = |ids: &[i64]| {
                ids.iter()
                    .map(|id| self.person_name(*id))
                    .collect::<Vec<_>>()
                    .join(" und ")
            };
            let (question, no_label) = match s.kind {
                SuggestionKind::SiblingParents => {
                    let target = s.targets[0];
                    let has_parent = !family.parents_of(target).is_empty();
                    (
                        format!(
                            "Sind {} auch Eltern von {}? ({} und {} gelten als Geschwister, erfasst sind aber nicht dieselben Eltern.)",
                            names(&s.candidates),
                            self.person_name(target),
                            names(&s.via),
                            self.person_name(target)
                        ),
                        if has_parent {
                            "Nein – Halbgeschwister"
                        } else {
                            "Nein"
                        },
                    )
                }
                SuggestionKind::PartnerParent => (
                    format!(
                        "Ist {} (Partner von {}) auch Elternteil von {}?",
                        names(&s.candidates),
                        names(&s.via),
                        names(&s.targets)
                    ),
                    "Nein – Stiefbeziehung",
                ),
            };
            ui.group(|ui| {
                ui.label(question);
                ui.horizontal_wrapped(|ui| {
                    let yes = if s.candidates.len() > 1 {
                        "Ja, beide"
                    } else {
                        "Ja"
                    };
                    if ui.add_enabled(!busy, egui::Button::new(yes)).clicked() {
                        answered = Some((s.clone(), Answer::All));
                    }
                    if s.candidates.len() > 1 {
                        for &c in &s.candidates {
                            if ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(format!(
                                        "Nur {} (Halbgeschwister)",
                                        self.person_name(c)
                                    )),
                                )
                                .clicked()
                            {
                                answered = Some((s.clone(), Answer::Only(c)));
                            }
                        }
                    }
                    if ui.add_enabled(!busy, egui::Button::new(no_label)).clicked() {
                        answered = Some((s.clone(), Answer::None));
                    }
                    if ui.button("Später").clicked() {
                        snooze = Some(format!("{s:?}"));
                    }
                });
            });
        }
        if let Some(key) = snooze {
            self.family_ui.snoozed.insert(key);
        }
        if let Some((suggestion, answer)) = answered {
            let root = self.settings.collection.clone();
            self.undo.clear();
            self.launch("Rückfrage beantworten", true, "mutation", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                gaps::apply(&mut c, &suggestion, answer)?;
                Ok(Outcome::Done("Antwort gespeichert".into()))
            });
        }
        ui.separator();
    }

    /// Anzahl der Rückfragen, die nicht zurückgestellt sind (für den Reiter).
    pub(super) fn open_questions(&self) -> usize {
        self.face_page
            .questions
            .iter()
            .filter(|s| !self.family_ui.snoozed.contains(&format!("{s:?}")))
            .count()
    }

    /// Formular „Person hinzufügen“ (ohne Gesichtsbezug, z. B. verstorbene Großeltern).
    pub(super) fn add_person_form(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let mut state = std::mem::take(&mut self.family_ui);
        egui::CollapsingHeader::new("Person hinzufügen (ohne Foto)")
            .id_salt("add-person")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut state.new_name).desired_width(200.0));
                    egui::ComboBox::from_id_salt("new-gender")
                        .selected_text(match state.new_gender.as_str() {
                            "weiblich" => "weiblich",
                            "männlich" => "männlich",
                            _ => "divers / unbekannt",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.new_gender, String::new(), "divers / unbekannt");
                            ui.selectable_value(&mut state.new_gender, "weiblich".into(), "weiblich");
                            ui.selectable_value(&mut state.new_gender, "männlich".into(), "männlich");
                        });
                    ui.label("Geburtsjahr");
                    ui.add(egui::TextEdit::singleline(&mut state.new_birth).desired_width(60.0));
                    ui.label("Notiz");
                    ui.add(egui::TextEdit::singleline(&mut state.new_note).desired_width(200.0));
                });
                let birth = state.new_birth.trim();
                let year = if birth.is_empty() { Ok(None) } else { birth.parse::<i32>().map(Some) };
                let new = faces::NewPerson {
                    name: state.new_name.clone(),
                    gender: state.new_gender.clone(),
                    birth_year: year.clone().unwrap_or(None),
                    note: state.new_note.clone(),
                };
                let problem = if year.is_err() {
                    Some("Das Geburtsjahr muss eine Zahl sein".to_string())
                } else if state.new_name.trim().is_empty() {
                    None
                } else {
                    faces::validate_new_person(&new).err().map(|e| e.to_string())
                };
                if let Some(problem) = &problem {
                    ui.colored_label(ui.visuals().warn_fg_color, problem);
                }
                let same = faces::people_named(&self.face_page.people, &state.new_name);
                if !same.is_empty() && !state.new_name.trim().is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "Hinweis: Es gibt schon {} Person(en) mit diesem Namen (#{}). Das ist erlaubt; die IDs bleiben unterscheidbar.",
                            same.len(),
                            same.iter().map(i64::to_string).collect::<Vec<_>>().join(", #")
                        ))
                        .weak(),
                    );
                }
                let ready = problem.is_none() && !state.new_name.trim().is_empty();
                if ui
                    .add_enabled(!busy && ready, egui::Button::new("Person anlegen"))
                    .clicked()
                {
                    let root = self.settings.collection.clone();
                    self.undo.clear();
                    self.launch("Person anlegen", true, "mutation", move |_| {
                        let mut c = Collection::open(Path::new(&root), true)?;
                        Ok(Outcome::PersonAdded(faces::add_person(&mut c, &new)?))
                    });
                    state.new_name.clear();
                    state.new_birth.clear();
                    state.new_note.clear();
                }
            });
        self.family_ui = state;
        if let Some(id) = self.family_ui.just_added
            && self.face_page.people.iter().any(|p| p.id == id)
        {
            ui.group(|ui| {
                ui.label(format!(
                    "{} wurde angelegt. Beziehung hinzufügen:",
                    self.person_name(id)
                ));
                self.quick_relation_row(ui, id);
                if ui.small_button("Fertig").clicked() {
                    self.family_ui.just_added = None;
                }
            });
        }
    }
}
